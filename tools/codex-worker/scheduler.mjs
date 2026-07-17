import { WorkerError } from "./protocol.mjs";

export class Scheduler {
  constructor(concurrency, maxQueue) {
    this.concurrency = concurrency;
    this.maxQueue = maxQueue;
    this.active = 0;
    this.queue = [];
    this.accepting = true;
    this.idleWaiters = [];
  }

  submit(task, signal) {
    if (!this.accepting) {
      return Promise.reject(new WorkerError("worker_shutting_down", 503));
    }
    if (signal?.aborted) {
      return Promise.reject(abortReason(signal));
    }
    if (this.active < this.concurrency) {
      return this.start(task, signal);
    }
    if (this.queue.length >= this.maxQueue) {
      return Promise.reject(new WorkerError("queue_full", 429));
    }
    return new Promise((resolvePromise, rejectPromise) => {
      const entry = {
        task,
        resolvePromise,
        rejectPromise,
        signal,
        abort: null,
      };
      entry.abort = () => {
        const index = this.queue.indexOf(entry);
        if (index === -1) {
          return;
        }
        this.queue.splice(index, 1);
        this.detach(entry);
        rejectPromise(abortReason(signal));
        this.resolveIdle();
      };
      signal?.addEventListener("abort", entry.abort, { once: true });
      this.queue.push(entry);
    });
  }

  start(task, signal) {
    this.active += 1;
    return Promise.resolve()
      .then(() => {
        if (signal?.aborted) {
          throw abortReason(signal);
        }
        return task();
      })
      .finally(() => this.release());
  }

  release() {
    this.active -= 1;
    let next = this.queue.shift();
    while (next?.signal?.aborted) {
      this.detach(next);
      next.rejectPromise(abortReason(next.signal));
      next = this.queue.shift();
    }
    if (next !== undefined) {
      this.detach(next);
      this.start(next.task, next.signal).then(next.resolvePromise, next.rejectPromise);
      return;
    }
    this.resolveIdle();
  }

  stop() {
    this.accepting = false;
    const queued = this.queue.splice(0);
    queued.forEach((entry) => {
      this.detach(entry);
      entry.rejectPromise(new WorkerError("worker_shutting_down", 503));
    });
    this.resolveIdle();
  }

  idle() {
    if (this.active === 0 && this.queue.length === 0) {
      return Promise.resolve();
    }
    return new Promise((resolvePromise) => this.idleWaiters.push(resolvePromise));
  }

  detach(entry) {
    entry.signal?.removeEventListener("abort", entry.abort);
  }

  resolveIdle() {
    if (this.active !== 0 || this.queue.length !== 0) {
      return;
    }
    const waiters = this.idleWaiters.splice(0);
    waiters.forEach((resolvePromise) => resolvePromise());
  }
}

export class RequestCounters {
  constructor() {
    this.accepted = 0;
    this.settled = 0;
  }

  submit(scheduler, task, signal) {
    if (this.accepted === Number.MAX_SAFE_INTEGER) {
      return Promise.reject(new WorkerError("request_counter_exhausted", 503));
    }
    this.accepted += 1;
    let submitted;
    try {
      submitted = scheduler.submit(task, signal);
    } catch (error) {
      this.settled += 1;
      throw error;
    }
    return Promise.resolve(submitted).finally(() => {
      this.settled += 1;
    });
  }

  snapshot(active, queued) {
    if (!Number.isSafeInteger(this.accepted)
      || !Number.isSafeInteger(this.settled)
      || this.accepted < 0
      || this.settled < 0
      || this.settled > this.accepted
      || this.accepted - this.settled !== active + queued) {
      throw new WorkerError("invalid_request_counters", 500);
    }
    return {
      accepted: this.accepted,
      settled: this.settled,
    };
  }
}

export function abortReason(signal) {
  return signal?.reason instanceof WorkerError
    ? signal.reason
    : new WorkerError("client_disconnected", 499);
}
