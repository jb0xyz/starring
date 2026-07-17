import assert from "node:assert/strict";
import test from "node:test";
import { AdmissionRegistry, validateObservationId } from "./admission-registry.mjs";

const FIRST = "11111111-1111-4111-8111-111111111111";
const SECOND = "22222222-2222-4222-8222-222222222222";

test("admission registry exposes only live admitted request state", () => {
  let now = 1_000;
  const registry = new AdmissionRegistry({ clock: () => now, ttlMs: 10, capacity: 2 });
  registry.reserve(FIRST, "internal-request-1");
  assert.equal(registry.lookup(FIRST), null);
  registry.admit(FIRST, "queued");
  assert.deepEqual(registry.lookup(FIRST), {
    schema_version: 1,
    observation_id: FIRST,
    status: "queued",
    request_id: "internal-request-1",
  });
  registry.activate(FIRST);
  assert.equal(registry.lookup(FIRST).status, "active");
  registry.release(FIRST);
  assert.equal(registry.lookup(FIRST), null);
  assert.throws(
    () => registry.reserve(FIRST, "internal-request-2"),
    (error) => error.code === "observation_id_collision" && error.status === 409,
  );
  now += 11;
  registry.reserve(FIRST, "internal-request-3");
  registry.admit(FIRST, "active");
  assert.equal(registry.lookup(FIRST).request_id, "internal-request-3");
});

test("admission registry fails closed at its fixed capacity", () => {
  const registry = new AdmissionRegistry({ capacity: 1 });
  registry.reserve(FIRST, "internal-request-1");
  assert.throws(
    () => registry.reserve(SECOND, "internal-request-2"),
    (error) => error.code === "observation_registry_full" && error.status === 503,
  );
  registry.release(FIRST);
  assert.throws(
    () => registry.reserve(SECOND, "internal-request-2"),
    (error) => error.code === "observation_registry_full" && error.status === 503,
  );
});

test("observation ids accept only canonical UUID v4 values", () => {
  assert.equal(validateObservationId(FIRST), FIRST);
  for (const invalid of [
    "11111111-1111-1111-8111-111111111111",
    "11111111-1111-4111-7111-111111111111",
    "11111111-1111-4111-8111-11111111111A",
    "secret/value",
    "",
  ]) {
    assert.throws(
      () => validateObservationId(invalid),
      (error) => error.code === "invalid_observation_id" && error.status === 400,
    );
  }
});
