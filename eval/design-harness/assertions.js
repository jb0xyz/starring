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
  if (report.schema_version !== 1) {
    throw new Error(`invalid eval report: unsupported schema_version ${report.schema_version}`);
  }
  for (const field of ['completed', 'final_validate_passed', 'final_simulate_passed']) {
    if (typeof report[field] !== 'boolean') {
      throw new Error(`invalid eval report: missing boolean ${field}`);
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

function checked(output, assertion) {
  try {
    return assertion(parseReport(output));
  } catch (error) {
    return result(false, error.message);
  }
}

function terminalOutcome(output, context) {
  return checked(output, (report) => {
    const scenario = vars(context).caseId;
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
    if (!report.final_validate_passed) {
      return result(false, `final validation failed: ${JSON.stringify(report.final_validate_error)}`);
    }
    if (expected.requireSimulation === true && !report.final_simulate_passed) {
      return result(false, `required simulation failed: ${JSON.stringify(report.final_simulate_error)}`);
    }
    return result(true, expected.requireSimulation === true ? 'validation and simulation passed' : 'validation passed');
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

function taskSemantics(output, context) {
  return checked(output, (report) => {
    const scenario = vars(context).caseId;
    const expected = scenario === 'studyroom_full'
      ? expectedStudyRoom()
      : scenario === 'simple_modal_ack'
        ? expectedSimpleModal()
        : null;
    if (!expected) {
      return result(false, `unknown caseId ${scenario}`);
    }
    const actual = structuredClone(report.ruleset);
    if (scenario === 'studyroom_full') {
      const field = actual.modals?.find((modal) => modal.key === 'study_modal')?.fields?.find((value) => value.key === 'room_name');
      if (field) {
        field.label = 'Room name';
      }
      for (const action of actual.rules?.flatMap((rule) => rule.actions || []) || []) {
        if (action.type === 'post_panel' && action.key === 'welcome_panel') {
          action.content = 'Welcome to ${input.room_name}';
        }
        if (action.type === 'post_panel' && action.key === 'hub_panel') {
          action.content = '${input.room_name} is open';
        }
      }
    }
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

module.exports = {
  callBudgets,
  distinctMutationTools,
  draftShape,
  finalGates,
  noExcessiveRepeatedErrors,
  parseReport,
  taskSemantics,
  terminalOutcome,
};
