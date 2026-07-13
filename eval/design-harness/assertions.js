const { isDeepStrictEqual } = require('node:util');

function parseReport(output) {
  let report;
  try {
    report = typeof output === 'string' ? JSON.parse(output) : output;
  } catch (error) {
    throw new Error(`invalid eval report: ${error.message}`);
  }
  if (!report || typeof report !== 'object' || Array.isArray(report)) {
    throw new Error('invalid eval report: expected an object');
  }
  if (![1, 2].includes(report.schema_version)) {
    throw new Error(`invalid eval report: unsupported schema_version ${report.schema_version}`);
  }
  if (typeof report.completed !== 'boolean') {
    throw new Error('invalid eval report: missing boolean completed');
  }
  if (report.schema_version === 1) {
    for (const field of ['final_validate_passed', 'final_simulate_passed']) {
      if (typeof report[field] !== 'boolean') {
        throw new Error(`invalid eval report: missing boolean ${field}`);
      }
    }
  } else {
    if (!Array.isArray(report.turns) || report.turns.length === 0) {
      throw new Error('invalid eval report: missing non-empty turns');
    }
    for (const field of ['actual_gates', 'postcheck']) {
      if (!report[field] || typeof report[field] !== 'object' || Array.isArray(report[field])) {
        throw new Error(`invalid eval report: missing object ${field}`);
      }
    }
    for (const field of ['validate_passed', 'simulate_attempted', 'simulate_passed']) {
      if (typeof report.postcheck[field] !== 'boolean') {
        throw new Error(`invalid eval report: missing boolean postcheck.${field}`);
      }
    }
  }
  if (typeof report.outcome !== 'string') {
    throw new Error('invalid eval report: missing string outcome');
  }
  for (const field of ['draft', 'ruleset', 'observability']) {
    if (!report[field] || typeof report[field] !== 'object' || Array.isArray(report[field])) {
      throw new Error(`invalid eval report: missing object ${field}`);
    }
  }
  return report;
}

function vars(context) {
  return context?.vars || context?.test?.vars || {};
}

function result(pass, reason, score = pass ? 1 : 0) {
  return { pass, score, reason };
}

function listVar(value) {
  if (Array.isArray(value)) {
    return value;
  }
  if (typeof value === 'string') {
    return value.split(',').map((entry) => entry.trim()).filter(Boolean);
  }
  return [];
}

function checked(output, assertion) {
  try {
    return assertion(parseReport(output));
  } catch (error) {
    return result(false, error.message);
  }
}

function terminalOutcome(output, context) {
  return checked(output, (report) => {
    const expected = vars(context);
    const expectedOutcomes = listVar(expected.expectedOutcomes);
    if (expectedOutcomes.length > 0) {
      const pass = expectedOutcomes.includes(report.outcome);
      return result(pass, pass
        ? `terminal outcome=${report.outcome}`
        : `expected one of ${expectedOutcomes.join(', ')}, received ${report.outcome}`);
    }
    const scenario = expected.caseId;
    if (scenario === 'studyroom_full') {
      const pass = report.outcome === 'completed' && report.completed === true;
      return result(pass, pass ? 'session completed' : `expected completed outcome, received ${report.outcome}`);
    }
    if (scenario === 'simple_modal_ack') {
      const expectedQuestion = 'Validation passed; stop this benchmark?';
      const pass = report.outcome === 'awaiting_human'
        && report.completed === false
        && report.question === expectedQuestion;
      return result(pass, pass ? 'session ended at the expected question' : `unexpected terminal state outcome=${report.outcome} question=${report.question}`);
    }
    return result(false, `unknown caseId ${scenario}`);
  });
}

function finalGates(output, context) {
  return checked(output, (report) => {
    const expected = vars(context);
    const validatePassed = report.schema_version === 1
      ? report.final_validate_passed
      : report.postcheck.validate_passed;
    const validateError = report.schema_version === 1
      ? report.final_validate_error
      : report.postcheck.validate_error;
    const simulatePassed = report.schema_version === 1
      ? report.final_simulate_passed
      : report.postcheck.simulate_passed;
    const simulateError = report.schema_version === 1
      ? report.final_simulate_error
      : report.postcheck.simulate_error;
    if (!validatePassed) {
      return result(false, `final validation failed: ${JSON.stringify(validateError)}`);
    }
    if (expected.requireSimulation === true && !simulatePassed) {
      return result(false, `required simulation failed: ${JSON.stringify(simulateError)}`);
    }
    return result(true, expected.requireSimulation === true ? 'validation and simulation passed' : 'validation passed');
  });
}

function actualGateStamps(output, context) {
  return checked(output, (report) => {
    const expected = vars(context);
    const actual = report.schema_version === 1
      ? {
        validation_current: report.validation_current,
        simulation_current: report.simulation_current,
      }
      : report.actual_gates;
    const failures = [];
    if (expected.requireActualValidation === true && actual.validation_current !== true) {
      failures.push('validation is not current');
    }
    if (expected.requireActualSimulation === true && actual.simulation_current !== true) {
      failures.push('simulation is not current');
    }
    return result(failures.length === 0, failures.length === 0 ? 'required actual gate stamps are current' : failures.join(', '));
  });
}

function conversationFlow(output, context) {
  return checked(output, (report) => {
    const expected = vars(context);
    if (report.schema_version !== 2) {
      return result(false, 'stateful conversation assertions require schema_version 2');
    }
    const turns = report.turns;
    const failures = [];
    if (Number.isInteger(expected.inputTurnCount) && turns.length !== expected.inputTurnCount) {
      failures.push(`turns=${turns.length} expected=${expected.inputTurnCount}`);
    }
    if (turns.some((turn) => turn.outcome === 'halted')) {
      failures.push('conversation halted');
    }
    if (expected.firstTurnNeedsInput === true && !['awaiting_human', 'needs_input'].includes(turns[0]?.outcome)) {
      failures.push(`first outcome=${turns[0]?.outcome} expected needs_input`);
    }
    if (expected.firstTurnNeedsInput === true && typeof turns[0]?.question !== 'string') {
      failures.push('first turn did not include a question');
    }
    if (expected.requireDraftUnchanged === true && turns[0]?.draft_changed !== false) {
      failures.push('clarification turn changed the Draft');
    }
    for (let index = 1; index < turns.length; index += 1) {
      if (turns[index - 1].draft_revision_after !== turns[index].draft_revision_before) {
        failures.push(`revision discontinuity at turn ${index + 1}`);
      }
    }
    const changedTurns = turns.filter((turn) => turn.draft_changed === true).length;
    if (Number.isInteger(expected.minChangedTurns) && changedTurns < expected.minChangedTurns) {
      failures.push(`changed_turns=${changedTurns} minimum=${expected.minChangedTurns}`);
    }
    const lastMutationCalls = turns.at(-1)?.observability_delta?.mutation_tool_calls || {};
    for (const tool of listVar(expected.requiredLastTurnMutationTools)) {
      if (!Number.isInteger(lastMutationCalls[tool]) || lastMutationCalls[tool] < 1) {
        failures.push(`last turn did not use required mutation tool ${tool}`);
      }
    }
    for (const tool of listVar(expected.forbiddenLastTurnMutationTools)) {
      if (Number.isInteger(lastMutationCalls[tool]) && lastMutationCalls[tool] > 0) {
        failures.push(`last turn used forbidden mutation tool ${tool}`);
      }
    }
    return result(failures.length === 0, failures.length === 0 ? `conversation processed ${turns.length} turns continuously` : failures.join(', '));
  });
}

function draftShape(output, context) {
  return checked(output, (report) => {
    const expected = vars(context);
    const summary = report.draft;
    const checks = [
      ['panels', expected.expectedPanels],
      ['modals', expected.expectedModals],
      ['rules', expected.expectedRules],
      ['actions', expected.expectedActions],
    ];
    const failures = checks
      .filter(([name, value]) => Number.isInteger(value) && summary[name] !== value)
      .map(([name, value]) => `${name}=${summary[name]} expected=${value}`);
    if (!Array.isArray(summary.unresolved_references) || summary.unresolved_references.length > 0) {
      failures.push(`unresolved=${JSON.stringify(summary.unresolved_references)}`);
    }
    return result(failures.length === 0, failures.length === 0 ? 'draft shape matches' : failures.join(', '));
  });
}

function expectedStudyRoom() {
  return {
    version: 1,
    panels: [{
      key: 'study_panel',
      channel: 'study_hub',
      content: 'Create a study room',
      buttons: [{ label: 'Create room', route: { static: { key: 'create_study_room' } } }],
    }],
    modals: [{
      key: 'study_modal',
      title: 'Create study room',
      fields: [{ key: 'room_name', label: 'Room name', style: 'short', required: true }],
    }],
    rules: [
      {
        key: 'open_modal',
        trigger: { type: 'button_click', component: 'create_study_room' },
        actions: [{ type: 'open_modal', modal: 'study_modal' }],
      },
      {
        key: 'submit_room',
        trigger: { type: 'modal_submit', modal: 'study_modal' },
        actions: [
          { type: 'defer_ephemeral' },
          { type: 'create_role', key: 'member_role', name: '${input.room_name} members' },
          { type: 'create_channel', key: 'room_channel', name: 'study-${input.room_name}' },
          { type: 'upsert_overwrite', channel: { created: 'room_channel' }, target: 'everyone', allow: '0', deny: '1024' },
          { type: 'upsert_overwrite', channel: { created: 'room_channel' }, target: { role: { created: 'member_role' } }, allow: '1024', deny: '0' },
          { type: 'grant_role', role: { created: 'member_role' }, target: 'actor' },
          {
            type: 'post_panel',
            key: 'welcome_panel',
            channel: { created: 'room_channel' },
            content: 'Welcome to ${input.room_name}',
            buttons: [
              { label: 'Help', route: { static: { key: 'study_help' } } },
              { label: 'Close', route: { instance_action: { instance: { created: 'study_instance' }, action: 'close' } } },
            ],
          },
          {
            type: 'post_panel',
            key: 'hub_panel',
            channel: 'study_hub',
            content: '${input.room_name} is open',
            buttons: [{ label: 'Join', route: { instance_action: { instance: { created: 'study_instance' }, action: 'join' } } }],
          },
          {
            type: 'register_instance',
            key: 'study_instance',
            kind: 'study_room',
            resources: {
              roles: { member_role: { created: 'member_role' } },
              channels: { room_channel: { created: 'room_channel' } },
              messages: {
                hub_panel: { created: 'hub_panel' },
                welcome_panel: { created: 'welcome_panel' },
              },
            },
          },
          { type: 'edit_response', content: 'Created ${input.room_name}' },
        ],
      },
    ],
  };
}

function expectedSimpleModal() {
  return {
    version: 1,
    panels: [],
    modals: [{
      key: 'feedback_modal',
      title: 'Feedback',
      fields: [{ key: 'message', label: 'Message', style: 'paragraph', required: true }],
    }],
    rules: [{
      key: 'ack_feedback',
      trigger: { type: 'modal_submit', modal: 'feedback_modal' },
      actions: [
        { type: 'defer_ephemeral' },
        { type: 'edit_response', content: 'Thanks, ${input.message}' },
      ],
    }],
  };
}

function expectedAdditiveRevision() {
  const ruleset = expectedSimpleModal();
  ruleset.panels.push({
    key: 'welcome_panel',
    channel: 'study_hub',
    content: 'Welcome',
    buttons: [{ label: 'Greet', route: { static: { key: 'greet' } } }],
  });
  ruleset.rules.push({
    key: 'greet_user',
    trigger: { type: 'button_click', component: 'greet' },
    actions: [
      { type: 'defer_ephemeral' },
      { type: 'edit_response', content: 'Hello!' },
    ],
  });
  return ruleset;
}

function expectedReplacementRevision() {
  const ruleset = expectedSimpleModal();
  ruleset.modals[0].title = 'Suggestions';
  ruleset.modals[0].fields[0].label = 'Details';
  ruleset.rules[0].actions[1].content = 'Received: ${input.message}';
  return ruleset;
}

function taskSemantics(output, context) {
  return checked(output, (report) => {
    const scenario = vars(context).caseId;
    const expected = ['studyroom_full', 'studyroom_incremental'].includes(scenario)
      ? expectedStudyRoom()
      : ['simple_modal_ack', 'complete_one_shot', 'multi_turn_elaboration'].includes(scenario)
        ? expectedSimpleModal()
        : scenario === 'additive_revision'
          ? expectedAdditiveRevision()
          : scenario === 'replacement_revision'
            ? expectedReplacementRevision()
            : null;
    if (!expected) {
      return result(false, `unknown caseId ${scenario}`);
    }
    const actual = structuredClone(report.ruleset);
    const pass = isDeepStrictEqual(actual, expected);
    return result(pass, pass ? `${scenario} semantics match` : `${scenario} ruleset does not exactly match`);
  });
}

function distinctMutationTools(output, context) {
  return checked(output, (report) => {
    const expected = vars(context);
    const tools = report.observability.distinct_mutation_tools;
    const count = Array.isArray(tools) ? tools.length : 0;
    return result(count >= expected.minDistinctMutationTools, `distinct mutation tools=${count} minimum=${expected.minDistinctMutationTools}`, Math.min(1, count / expected.minDistinctMutationTools));
  });
}

function noExcessiveRepeatedErrors(output) {
  return checked(output, (report) => {
    const maximum = report.max_repeat_count;
    const pass = Number.isInteger(maximum) && maximum <= 2;
    return result(pass, `maximum identical error count=${maximum}`);
  });
}

function callBudgets(output, context) {
  return checked(output, (report) => {
    const expected = vars(context);
    const modelCalls = report.observability.model_calls;
    const toolCalls = report.observability.tool_calls;
    const pass = Number.isInteger(modelCalls)
      && Number.isInteger(toolCalls)
      && modelCalls <= expected.maxModelCalls
      && toolCalls <= expected.maxToolCalls;
    return result(pass, `model_calls=${modelCalls}/${expected.maxModelCalls} tool_calls=${toolCalls}/${expected.maxToolCalls}`);
  });
}

function perTurnBudgets(output, context) {
  return checked(output, (report) => {
    const expected = vars(context);
    if (report.schema_version !== 2) {
      return result(false, 'per-turn budgets require schema_version 2');
    }
    const failures = report.turns.flatMap((turn) => {
      const delta = turn.observability_delta || {};
      const over = [];
      if (!Number.isInteger(delta.model_calls) || delta.model_calls > expected.maxModelCallsPerTurn) {
        over.push(`model_calls=${delta.model_calls}/${expected.maxModelCallsPerTurn}`);
      }
      if (!Number.isInteger(delta.tool_calls) || delta.tool_calls > expected.maxToolCallsPerTurn) {
        over.push(`tool_calls=${delta.tool_calls}/${expected.maxToolCallsPerTurn}`);
      }
      return over.length === 0 ? [] : [`${turn.id}: ${over.join(' ')}`];
    });
    return result(failures.length === 0, failures.length === 0 ? 'all turns stayed within call budgets' : failures.join(', '));
  });
}

module.exports = {
  actualGateStamps,
  callBudgets,
  conversationFlow,
  distinctMutationTools,
  draftShape,
  finalGates,
  noExcessiveRepeatedErrors,
  parseReport,
  perTurnBudgets,
  taskSemantics,
  terminalOutcome,
};
