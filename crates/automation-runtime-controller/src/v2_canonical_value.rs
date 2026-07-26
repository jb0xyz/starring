use std::num::NonZeroU64;
use std::time::Duration;

use chrono::{DateTime, Utc};

const MIN_UNIX_MICROSECONDS: i64 = -62_135_596_800_000_000;
const MAX_UNIX_MICROSECONDS: i64 = 253_402_300_799_999_999;
const MIN_SERVING_LEASE_MILLISECONDS: u64 = 1_000;
const MAX_SERVING_LEASE_MILLISECONDS: u64 = 300_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeCanonicalValueErrorV2 {
    #[error("runtime canonical timestamp has sub-microsecond precision")]
    TimestampSubMicrosecond,
    #[error("runtime canonical timestamp uses a leap-second representation")]
    TimestampLeapSecond,
    #[error("runtime canonical timestamp is outside the supported range")]
    TimestampOutOfRange,
    #[error("runtime canonical serving lease has sub-millisecond precision")]
    ServingLeaseSubMillisecond,
    #[error("runtime canonical serving lease is outside the supported range")]
    ServingLeaseOutOfRange,
    #[error("runtime canonical persistence integer exceeds the database range")]
    PersistenceIntegerOutOfRange,
    #[error("runtime canonical Discord snowflake text is not canonical")]
    DiscordSnowflakeNonCanonical,
    #[error("runtime canonical Discord snowflake is outside the supported range")]
    DiscordSnowflakeOutOfRange,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RuntimeUnixMicrosecondsV2(i64);

impl RuntimeUnixMicrosecondsV2 {
    pub fn from_datetime(value: DateTime<Utc>) -> Result<Self, RuntimeCanonicalValueErrorV2> {
        let nanoseconds = value.timestamp_subsec_nanos();
        if nanoseconds >= 1_000_000_000 {
            return Err(RuntimeCanonicalValueErrorV2::TimestampLeapSecond);
        }
        if !nanoseconds.is_multiple_of(1_000) {
            return Err(RuntimeCanonicalValueErrorV2::TimestampSubMicrosecond);
        }
        let microseconds = value
            .timestamp()
            .checked_mul(1_000_000)
            .and_then(|seconds| seconds.checked_add(i64::from(nanoseconds / 1_000)))
            .ok_or(RuntimeCanonicalValueErrorV2::TimestampOutOfRange)?;
        Self::from_i64(microseconds)
    }

    pub const fn from_i64(value: i64) -> Result<Self, RuntimeCanonicalValueErrorV2> {
        if value < MIN_UNIX_MICROSECONDS || value > MAX_UNIX_MICROSECONDS {
            return Err(RuntimeCanonicalValueErrorV2::TimestampOutOfRange);
        }
        Ok(Self(value))
    }

    pub const fn get(self) -> i64 {
        self.0
    }

    pub fn to_datetime(self) -> DateTime<Utc> {
        DateTime::from_timestamp_micros(self.0)
            .expect("validated runtime Unix microseconds must convert to UTC")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RuntimeServingLeaseMillisecondsV2(u64);

impl RuntimeServingLeaseMillisecondsV2 {
    pub fn from_duration(value: Duration) -> Result<Self, RuntimeCanonicalValueErrorV2> {
        if !value.subsec_nanos().is_multiple_of(1_000_000) {
            return Err(RuntimeCanonicalValueErrorV2::ServingLeaseSubMillisecond);
        }
        let milliseconds = value.as_millis();
        if milliseconds < u128::from(MIN_SERVING_LEASE_MILLISECONDS)
            || milliseconds > u128::from(MAX_SERVING_LEASE_MILLISECONDS)
        {
            return Err(RuntimeCanonicalValueErrorV2::ServingLeaseOutOfRange);
        }
        Ok(Self(milliseconds as u64))
    }

    pub const fn from_milliseconds(value: u64) -> Result<Self, RuntimeCanonicalValueErrorV2> {
        if value < MIN_SERVING_LEASE_MILLISECONDS || value > MAX_SERVING_LEASE_MILLISECONDS {
            return Err(RuntimeCanonicalValueErrorV2::ServingLeaseOutOfRange);
        }
        Ok(Self(value))
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct RuntimePersistenceU64V2(u64);

impl RuntimePersistenceU64V2 {
    pub(crate) const fn from_u64(value: u64) -> Result<Self, RuntimeCanonicalValueErrorV2> {
        if value > i64::MAX as u64 {
            return Err(RuntimeCanonicalValueErrorV2::PersistenceIntegerOutOfRange);
        }
        Ok(Self(value))
    }

    pub(crate) const fn from_non_zero(
        value: NonZeroU64,
    ) -> Result<Self, RuntimeCanonicalValueErrorV2> {
        Self::from_u64(value.get())
    }

    pub(crate) const fn get_u64(self) -> u64 {
        self.0
    }

    pub(crate) const fn get_i64(self) -> i64 {
        self.0 as i64
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct RuntimeDiscordSnowflakeV2(u64);

impl RuntimeDiscordSnowflakeV2 {
    pub(crate) const fn from_u64(value: u64) -> Result<Self, RuntimeCanonicalValueErrorV2> {
        if value == 0 {
            return Err(RuntimeCanonicalValueErrorV2::DiscordSnowflakeOutOfRange);
        }
        Ok(Self(value))
    }

    pub(crate) fn parse_text(value: &str) -> Result<Self, RuntimeCanonicalValueErrorV2> {
        if value == "0" {
            return Err(RuntimeCanonicalValueErrorV2::DiscordSnowflakeOutOfRange);
        }
        if value.is_empty()
            || value.starts_with('0')
            || !value.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(RuntimeCanonicalValueErrorV2::DiscordSnowflakeNonCanonical);
        }
        let value = value
            .parse::<u64>()
            .map_err(|_| RuntimeCanonicalValueErrorV2::DiscordSnowflakeOutOfRange)?;
        Self::from_u64(value)
    }

    pub(crate) const fn get_u64(self) -> u64 {
        self.0
    }

    pub(crate) fn canonical_text(self) -> String {
        self.0.to_string()
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;
    use std::time::Duration;

    use chrono::{DateTime, Utc};

    use super::{
        RuntimeCanonicalValueErrorV2, RuntimeDiscordSnowflakeV2, RuntimePersistenceU64V2,
        RuntimeServingLeaseMillisecondsV2, RuntimeUnixMicrosecondsV2, MAX_UNIX_MICROSECONDS,
        MIN_UNIX_MICROSECONDS,
    };

    #[test]
    fn unix_microseconds_accept_the_exact_supported_boundaries() {
        for value in [MIN_UNIX_MICROSECONDS, -1, 0, 1, MAX_UNIX_MICROSECONDS] {
            let canonical = RuntimeUnixMicrosecondsV2::from_i64(value).unwrap();
            assert_eq!(canonical.get(), value);
            assert_eq!(
                RuntimeUnixMicrosecondsV2::from_datetime(canonical.to_datetime()).unwrap(),
                canonical
            );
        }
    }

    #[test]
    fn unix_microseconds_reject_values_adjacent_to_the_supported_range() {
        assert_eq!(
            RuntimeUnixMicrosecondsV2::from_i64(MIN_UNIX_MICROSECONDS - 1),
            Err(RuntimeCanonicalValueErrorV2::TimestampOutOfRange)
        );
        assert_eq!(
            RuntimeUnixMicrosecondsV2::from_i64(MAX_UNIX_MICROSECONDS + 1),
            Err(RuntimeCanonicalValueErrorV2::TimestampOutOfRange)
        );

        for value in [MIN_UNIX_MICROSECONDS - 1, MAX_UNIX_MICROSECONDS + 1] {
            let timestamp = DateTime::<Utc>::from_timestamp_micros(value).unwrap();
            assert_eq!(
                RuntimeUnixMicrosecondsV2::from_datetime(timestamp),
                Err(RuntimeCanonicalValueErrorV2::TimestampOutOfRange)
            );
        }
    }

    #[test]
    fn negative_fraction_uses_unix_floor_semantics() {
        let value = DateTime::<Utc>::from_timestamp(-1, 999_999_000).unwrap();
        let canonical = RuntimeUnixMicrosecondsV2::from_datetime(value).unwrap();

        assert_eq!(canonical.get(), -1);
        assert_eq!(canonical.to_datetime(), value);
    }

    #[test]
    fn timestamp_precision_and_leap_second_fail_distinctly() {
        let sub_microsecond = DateTime::<Utc>::from_timestamp(0, 1).unwrap();
        assert_eq!(
            RuntimeUnixMicrosecondsV2::from_datetime(sub_microsecond),
            Err(RuntimeCanonicalValueErrorV2::TimestampSubMicrosecond)
        );

        let leap_second = DateTime::<Utc>::from_timestamp(59, 1_000_000_000).unwrap();
        assert_eq!(
            RuntimeUnixMicrosecondsV2::from_datetime(leap_second),
            Err(RuntimeCanonicalValueErrorV2::TimestampLeapSecond)
        );

        let imprecise_leap_second = DateTime::<Utc>::from_timestamp(59, 1_000_000_001).unwrap();
        assert_eq!(
            RuntimeUnixMicrosecondsV2::from_datetime(imprecise_leap_second),
            Err(RuntimeCanonicalValueErrorV2::TimestampLeapSecond)
        );
    }

    #[test]
    fn serving_lease_accepts_only_exact_whole_millisecond_boundaries() {
        for value in [1_000, 300_000] {
            let canonical = RuntimeServingLeaseMillisecondsV2::from_milliseconds(value).unwrap();
            assert_eq!(canonical.get(), value);
            assert_eq!(
                RuntimeServingLeaseMillisecondsV2::from_duration(Duration::from_millis(value))
                    .unwrap(),
                canonical
            );
        }

        for value in [0, 999, 300_001, u64::MAX] {
            assert_eq!(
                RuntimeServingLeaseMillisecondsV2::from_milliseconds(value),
                Err(RuntimeCanonicalValueErrorV2::ServingLeaseOutOfRange)
            );
        }
    }

    #[test]
    fn serving_lease_rejects_sub_millisecond_precision_before_range() {
        for value in [
            Duration::new(1, 1),
            Duration::new(300, 1),
            Duration::new(301, 1),
        ] {
            assert_eq!(
                RuntimeServingLeaseMillisecondsV2::from_duration(value),
                Err(RuntimeCanonicalValueErrorV2::ServingLeaseSubMillisecond)
            );
        }

        assert_eq!(
            RuntimeServingLeaseMillisecondsV2::from_duration(Duration::from_millis(300_001)),
            Err(RuntimeCanonicalValueErrorV2::ServingLeaseOutOfRange)
        );
        assert_eq!(
            RuntimeServingLeaseMillisecondsV2::from_duration(Duration::from_millis(999)),
            Err(RuntimeCanonicalValueErrorV2::ServingLeaseOutOfRange)
        );
        assert_eq!(
            RuntimeServingLeaseMillisecondsV2::from_duration(Duration::from_secs(u64::MAX)),
            Err(RuntimeCanonicalValueErrorV2::ServingLeaseOutOfRange)
        );
    }

    #[test]
    fn persistence_integer_matches_the_postgresql_bigint_range() {
        for value in [0, 1, i64::MAX as u64] {
            let canonical = RuntimePersistenceU64V2::from_u64(value).unwrap();
            assert_eq!(canonical.get_u64(), value);
            assert_eq!(canonical.get_i64(), value as i64);
        }

        for value in [i64::MAX as u64 + 1, u64::MAX] {
            assert_eq!(
                RuntimePersistenceU64V2::from_u64(value),
                Err(RuntimeCanonicalValueErrorV2::PersistenceIntegerOutOfRange)
            );
        }

        assert_eq!(
            RuntimePersistenceU64V2::from_non_zero(NonZeroU64::new(1).unwrap())
                .unwrap()
                .get_i64(),
            1
        );
        assert_eq!(
            RuntimePersistenceU64V2::from_non_zero(NonZeroU64::new(i64::MAX as u64).unwrap())
                .unwrap()
                .get_i64(),
            i64::MAX
        );
        assert_eq!(
            RuntimePersistenceU64V2::from_non_zero(NonZeroU64::new(i64::MAX as u64 + 1).unwrap()),
            Err(RuntimeCanonicalValueErrorV2::PersistenceIntegerOutOfRange)
        );
    }

    #[test]
    fn discord_snowflake_accepts_the_full_unsigned_range_as_canonical_text() {
        for (value, expected) in [
            (1, "1"),
            (i64::MAX as u64, "9223372036854775807"),
            (i64::MAX as u64 + 1, "9223372036854775808"),
            (u64::MAX, "18446744073709551615"),
        ] {
            let from_number = RuntimeDiscordSnowflakeV2::from_u64(value).unwrap();
            let from_text = RuntimeDiscordSnowflakeV2::parse_text(expected).unwrap();

            assert_eq!(from_number, from_text);
            assert_eq!(from_number.get_u64(), value);
            assert_eq!(from_number.canonical_text(), expected);
        }
    }

    #[test]
    fn discord_snowflake_rejects_zero_and_unsigned_overflow() {
        assert_eq!(
            RuntimeDiscordSnowflakeV2::from_u64(0),
            Err(RuntimeCanonicalValueErrorV2::DiscordSnowflakeOutOfRange)
        );
        for value in ["0", "18446744073709551616", "999999999999999999999"] {
            assert_eq!(
                RuntimeDiscordSnowflakeV2::parse_text(value),
                Err(RuntimeCanonicalValueErrorV2::DiscordSnowflakeOutOfRange)
            );
        }
    }

    #[test]
    fn discord_snowflake_rejects_noncanonical_decimal_text() {
        for value in [
            "", "00", "01", "+1", "-1", " 1", "1 ", "1\n", "1.0", "1_0", "1a", "１",
        ] {
            assert_eq!(
                RuntimeDiscordSnowflakeV2::parse_text(value),
                Err(RuntimeCanonicalValueErrorV2::DiscordSnowflakeNonCanonical)
            );
        }
    }
}
