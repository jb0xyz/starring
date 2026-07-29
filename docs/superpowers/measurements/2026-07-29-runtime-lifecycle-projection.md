# Runtime lifecycle projection diagnostic

Date: 2026-07-29 KST

Status: diagnostic evidence, not production certification

## Source and host

- source revision: `aaa07536b8619a325be6605e19cac44564dabed7`
- model: `Mac16,10`
- physical CPU cores: 10
- memory: 25,769,803,776 bytes
- macOS: 26.5.2
- profile: Cargo release, locked dependencies, incremental compilation disabled

## Method

The ignored
`runtime_direct_trip_projection_release_diagnostic_v2` test ran 20 warm-up
iterations followed by 200 recorded iterations. Every iteration created an
idle in-process runtime boundary and measured the ordered readiness seal,
maintenance-ingress seal, and gateway shutdown projection.

```text
cargo test --release --locked -p starring-runtime runtime_direct_trip_projection_release_diagnostic_v2 -- --ignored --nocapture --test-threads=1
```

Nearest-rank percentiles are reported in nanoseconds.

| Boundary | P50 | P95 | P99 | Maximum |
| --- | ---: | ---: | ---: | ---: |
| readiness seal | 541 | 667 | 750 | 1,167 |
| maintenance-ingress seal | 708 | 917 | 959 | 1,459 |
| gateway shutdown projection | 2,208 | 2,709 | 2,959 | 3,709 |

The test passed with 200 complete recorded samples for every boundary.

## Interpretation boundary

This cohort measures only the deterministic idle in-process projection path.
It does not measure OS signal delivery, Discord hard-pause acknowledgement,
PostgreSQL cleanup, launchd behavior, network delay, customer interaction
execution, or operation at 50% and 90% capacity. It is useful for detecting a
local projection regression and cannot support an availability, shutdown, or
commercial-load SLO claim.
