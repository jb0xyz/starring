import assert from "node:assert/strict";
import test from "node:test";
import { EXPECTED_IDENTITY } from "./plans.mjs";
import {
  WorkloadError,
  buildTransportRequest,
  getWorkload,
  validateProductResult,
  validateTransportCompletion,
} from "./workloads.mjs";

function usage() {
  return {
    input_tokens: 10,
    cached_input_tokens: 5,
    output_tokens: 3,
    reasoning_output_tokens: 1,
  };
}

function productCall(requestId, frontierName, callUsage = usage()) {
  return {
    request_id: requestId,
    completion_sha256: "c".repeat(64),
    status_code: 200,
    latency_ms: 12,
    ...EXPECTED_IDENTITY,
    frontier_name: frontierName,
    usage: callUsage,
  };
}

test("transport probe binds one unique sequence into one frontier", () => {
  const request = buildTransportRequest("run-0001");
  assert.equal(request.messages.length, 2);
  assert.equal(request.frontier.name, "record_slo_probe");
  assert.equal(request.frontier.parameters.properties.sequence.const, "run-0001");
  assert.deepEqual(request.frontier.parameters.required, [
    "schema_version",
    "sequence",
    "status",
  ]);
  assert.equal(request.frontier.parameters.additionalProperties, false);
  assert.throws(
    () => buildTransportRequest("bad sequence"),
    (error) => error instanceof WorkloadError && error.code === "invalid_probe_sequence",
  );
});

test("transport completion requires exact identity and arguments", () => {
  const body = {
    schema_version: 1,
    request_id: "request-1",
    completion_sha256: "c".repeat(64),
    ...EXPECTED_IDENTITY,
    tool_call: {
      id: "call-request-1",
      name: "record_slo_probe",
      arguments: JSON.stringify({ schema_version: 1, sequence: "run-0001", status: "ok" }),
    },
    usage: usage(),
    duration_ms: 11,
  };
  const valid = validateTransportCompletion(200, body, "run-0001");
  assert.equal(valid.request_id, "request-1");
  assert.equal(valid.completion_sha256, "c".repeat(64));
  assert.deepEqual(valid.usage, usage());
  const invalidCompletion = structuredClone(body);
  invalidCompletion.completion_sha256 = "invalid";
  assert.throws(
    () => validateTransportCompletion(200, invalidCompletion, "run-0001"),
    (error) => error instanceof WorkloadError && error.code === "transport_invalid_response",
  );
  body.tool_call.arguments = JSON.stringify({
    schema_version: 1,
    sequence: "wrong",
    status: "ok",
  });
  assert.throws(
    () => validateTransportCompletion(200, body, "run-0001"),
    (error) => error instanceof WorkloadError && error.code === "transport_wrong_arguments",
  );
});

test("product adapters must return the fixed V15 quality contract", () => {
  const workload = getWorkload("starring_v15_two_call");
  const result = validateProductResult(workload, {
    case_id: workload.case_id,
    calls: [
      productCall("request-core", "interpret_intent_core"),
      productCall("request-details", "extract_private_study_room_details"),
    ],
    exact_semantics: true,
    validation_current: true,
    simulation_current: true,
    candidate_only: true,
  });
  assert.equal(result.calls.length, 2);
  assert.throws(
    () => validateProductResult(workload, { ...result, exact_semantics: false }),
    (error) => error instanceof WorkloadError && error.code === "invalid_product_result",
  );
});

test("product call evidence retains exact unique request ids with zero-token usage", () => {
  const workload = getWorkload("starring_v15_two_call");
  const zeroUsage = {
    input_tokens: 0,
    cached_input_tokens: 0,
    output_tokens: 0,
    reasoning_output_tokens: 0,
  };
  const input = {
    case_id: workload.case_id,
    calls: [
      productCall("worker-request-001", "interpret_intent_core", zeroUsage),
      productCall("worker-request-002", "extract_private_study_room_details", zeroUsage),
    ],
    exact_semantics: true,
    validation_current: true,
    simulation_current: true,
    candidate_only: true,
  };
  const result = validateProductResult(workload, input);
  assert.deepEqual(
    result.calls.map((call) => call.request_id),
    ["worker-request-001", "worker-request-002"],
  );
  assert.deepEqual(result.calls.map((call) => call.usage), [zeroUsage, zeroUsage]);
  assert.throws(
    () => validateProductResult(workload, {
      ...input,
      calls: input.calls.map((call) => ({ ...call, request_id: "worker-request-001" })),
    }),
    (error) => error instanceof WorkloadError
      && error.code === "duplicate_product_request_id",
  );
  const missingRequestId = structuredClone(input);
  delete missingRequestId.calls[0].request_id;
  assert.throws(
    () => validateProductResult(workload, missingRequestId),
    (error) => error instanceof WorkloadError && error.code === "invalid_product_call",
  );
  const missingCompletion = structuredClone(input);
  delete missingCompletion.calls[0].completion_sha256;
  assert.throws(
    () => validateProductResult(workload, missingCompletion),
    (error) => error instanceof WorkloadError && error.code === "invalid_product_call",
  );
});
