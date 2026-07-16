use super::core::*;
use super::english::safety_control_action_effect_meaning;

pub(super) const MAX_KOREAN_CONTROL_CLAUSES: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::turn) enum KoreanSafetyControlClause {
    Control(SafetyControlMeaning),
    BusinessOperation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KoreanActionClass {
    Ha,
    Geonneottwi,
    Native,
}

pub(in crate::turn) fn closed_korean_safety_control_clause(
    words: &[&str],
) -> Option<KoreanSafetyControlClause> {
    if !words.contains(&"말고") {
        return closed_korean_safety_control_atom(words);
    }
    let mut start = 0usize;
    let mut clauses = 0usize;
    let mut aggregate = SafetyControlMeaning::PreservesControl;
    for connector in words
        .iter()
        .enumerate()
        .filter_map(|(index, word)| (*word == "말고").then_some(index))
    {
        if connector == start || connector.saturating_add(1) >= words.len() {
            return None;
        }
        clauses = clauses.saturating_add(1);
        if clauses >= MAX_KOREAN_CONTROL_CLAUSES {
            return None;
        }
        aggregate = merge_korean_control_meaning(
            aggregate,
            closed_korean_control_atom_meaning(&words[start..=connector])?,
        );
        start = connector.saturating_add(1);
    }
    aggregate = merge_korean_control_meaning(
        aggregate,
        closed_korean_control_atom_meaning(&words[start..])?,
    );
    Some(KoreanSafetyControlClause::Control(aggregate))
}

fn closed_korean_control_atom_meaning(words: &[&str]) -> Option<SafetyControlMeaning> {
    match closed_korean_safety_control_atom(words)? {
        KoreanSafetyControlClause::Control(meaning) => Some(meaning),
        KoreanSafetyControlClause::BusinessOperation => None,
    }
}

fn merge_korean_control_meaning(
    left: SafetyControlMeaning,
    right: SafetyControlMeaning,
) -> SafetyControlMeaning {
    if left == SafetyControlMeaning::WeakensControl || right == SafetyControlMeaning::WeakensControl
    {
        SafetyControlMeaning::WeakensControl
    } else {
        SafetyControlMeaning::PreservesControl
    }
}

fn closed_korean_safety_control_atom(words: &[&str]) -> Option<KoreanSafetyControlClause> {
    let words = words.strip_prefix(&["보안", "모드에서"]).unwrap_or(words);
    let target_length = korean_control_target_prefix_length(words)?;
    let remainder = strip_korean_control_modifiers(&words[target_length..]);
    if korean_safety_control_business_operation(remainder) {
        return Some(KoreanSafetyControlClause::BusinessOperation);
    }
    let meaning = korean_without_safety_control_meaning(remainder)
        .or_else(|| korean_nominal_safety_control_meaning(remainder))
        .or_else(|| korean_safety_control_state_meaning(remainder))
        .or_else(|| korean_direct_safety_control_meaning(remainder))?;
    Some(KoreanSafetyControlClause::Control(meaning))
}

fn strip_korean_control_modifiers<'a>(mut words: &'a [&'a str]) -> &'a [&'a str] {
    while words.first().is_some_and(|word| {
        matches!(
            *word,
            "항상" | "계속" | "그대로" | "반드시" | "모두" | "전부"
        )
    }) {
        words = &words[1..];
    }
    words
}

fn korean_safety_control_business_operation(words: &[&str]) -> bool {
    if words.len() < 2
        || !words[..words.len().saturating_sub(1)]
            .iter()
            .all(|word| korean_business_object(word))
    {
        return false;
    }
    korean_direct_action_effect(&words[words.len().saturating_sub(1)..]).is_some()
}

fn korean_business_object(word: &str) -> bool {
    korean_stem_with_particle(
        word,
        &[
            "기록",
            "로그",
            "지연",
            "애니메이션",
            "알림",
            "요청",
            "메시지",
        ],
    )
}

fn korean_without_safety_control_meaning(words: &[&str]) -> Option<SafetyControlMeaning> {
    if words.first() != Some(&"없이") {
        return None;
    }
    let negated = korean_process_negated(&words[1..])?;
    Some(if negated {
        SafetyControlMeaning::PreservesControl
    } else {
        SafetyControlMeaning::WeakensControl
    })
}

fn korean_process_negated(words: &[&str]) -> Option<bool> {
    for process in ["진행", "처리", "배포", "적용", "실행"] {
        let Some(suffix) = words.first()?.strip_prefix(process) else {
            continue;
        };
        if words.len() == 1
            && matches!(
                suffix,
                "" | "해" | "해줘" | "해주세요" | "하세요" | "한다" | "해야해" | "해야합니다"
            )
        {
            return Some(false);
        }
        let suffix = suffix.strip_prefix("하").unwrap_or(suffix);
        if korean_closed_negative_suffix(suffix, &words[1..]) {
            return Some(true);
        }
    }
    None
}

fn korean_nominal_safety_control_meaning(words: &[&str]) -> Option<SafetyControlMeaning> {
    let (effect, nominal) = korean_nominal_action_effect(words.first()?)?;
    let governance = &words[1..];
    let tail = if governance.is_empty() && nominal {
        SafetyControlTailEffect::Direct
    } else {
        korean_governance_tail(governance)?
    };
    Some(safety_control_action_effect_meaning(effect, tail, false))
}

fn korean_nominal_action_effect(word: &str) -> Option<(SafetyControlActionEffect, bool)> {
    for (stem, effect) in korean_action_stems() {
        if word == *stem {
            return Some((*effect, true));
        }
        if word
            .strip_prefix(stem)
            .is_some_and(|suffix| matches!(suffix, "을" | "를"))
        {
            return Some((*effect, false));
        }
    }
    if korean_stem_with_particle(word, &["건너뛰기"]) {
        return Some((SafetyControlActionEffect::WeakensControl, false));
    }
    None
}

fn korean_governance_tail(words: &[&str]) -> Option<SafetyControlTailEffect> {
    if words.len() == 1 && korean_ha_command(words[0], "허용")
        || korean_action_negated(words, "금지", KoreanActionClass::Ha)
    {
        return Some(SafetyControlTailEffect::Permitted);
    }
    if words.len() == 1 && korean_ha_command(words[0], "금지")
        || korean_action_negated(words, "허용", KoreanActionClass::Ha)
    {
        return Some(SafetyControlTailEffect::Prohibited);
    }
    None
}

fn korean_safety_control_state_meaning(words: &[&str]) -> Option<SafetyControlMeaning> {
    match words {
        ["필요해"] | ["필요합니다"] => Some(SafetyControlMeaning::PreservesControl),
        ["필요", "없어"]
        | ["필요", "없습니다"]
        | ["필요없어"]
        | ["필요없습니다"]
        | ["필요하지", "않아"]
        | ["필요하지", "않습니다"] => Some(SafetyControlMeaning::WeakensControl),
        ["선택", "사항으로", "해"]
        | ["선택", "사항으로", "해줘"]
        | ["선택", "사항으로", "해주세요"]
        | ["선택사항으로", "해"]
        | ["선택사항으로", "해줘"]
        | ["선택사항으로", "해주세요"] => Some(SafetyControlMeaning::WeakensControl),
        _ => None,
    }
}

fn korean_direct_safety_control_meaning(words: &[&str]) -> Option<SafetyControlMeaning> {
    if korean_forced_action(words) {
        let (effect, _) = korean_action_surface(words.first()?)?;
        return Some(safety_control_action_effect_meaning(
            effect,
            SafetyControlTailEffect::Direct,
            false,
        ));
    }
    if let Some((effect, class)) = korean_action_surface(words.first()?) {
        if korean_action_negated(words, korean_action_stem(words.first()?)?, class) {
            return Some(safety_control_action_effect_meaning(
                effect,
                SafetyControlTailEffect::Direct,
                true,
            ));
        }
    }
    let effect = korean_direct_action_effect(words)?;
    Some(safety_control_action_effect_meaning(
        effect,
        SafetyControlTailEffect::Direct,
        false,
    ))
}

fn korean_direct_action_effect(words: &[&str]) -> Option<SafetyControlActionEffect> {
    if words == ["건너뛴"] {
        return Some(SafetyControlActionEffect::WeakensControl);
    }
    let (effect, class) = korean_action_surface(words.first()?)?;
    let stem = korean_action_stem(words.first()?)?;
    let suffix = words.first()?.strip_prefix(stem)?;
    let direct = match class {
        KoreanActionClass::Ha => {
            words.len() == 1
                && matches!(
                    suffix,
                    "" | "해" | "해줘" | "해주세요" | "하세요" | "해야해" | "해야합니다" | "한다"
                )
        }
        KoreanActionClass::Geonneottwi => {
            words.len() == 1 && matches!(suffix, "" | "어" | "어줘" | "어주세요" | "세요" | "ㄴ")
        }
        KoreanActionClass::Native => {
            (words.len() == 1 && matches!(suffix, "" | "줘" | "주세요" | "둬"))
                || (stem == "켜" && words == ["켜", "둬"])
        }
    };
    direct.then_some(effect)
}

fn korean_action_surface(word: &str) -> Option<(SafetyControlActionEffect, KoreanActionClass)> {
    korean_action_stems().iter().find_map(|(stem, effect)| {
        word.strip_prefix(stem).map(|_| {
            let class = if *stem == "건너뛰" {
                KoreanActionClass::Geonneottwi
            } else if matches!(*stem, "꺼두" | "없애" | "빼" | "끄" | "꺼" | "켜") {
                KoreanActionClass::Native
            } else {
                KoreanActionClass::Ha
            };
            (*effect, class)
        })
    })
}

fn korean_action_stem(word: &str) -> Option<&'static str> {
    korean_action_stems()
        .iter()
        .find_map(|(stem, _)| word.strip_prefix(stem).map(|_| *stem))
}

fn korean_action_stems() -> &'static [(&'static str, SafetyControlActionEffect)] {
    &[
        ("비활성화", SafetyControlActionEffect::WeakensControl),
        ("건너뛰", SafetyControlActionEffect::WeakensControl),
        ("꺼두", SafetyControlActionEffect::WeakensControl),
        ("생략", SafetyControlActionEffect::WeakensControl),
        ("우회", SafetyControlActionEffect::WeakensControl),
        ("제거", SafetyControlActionEffect::WeakensControl),
        ("무시", SafetyControlActionEffect::WeakensControl),
        ("없애", SafetyControlActionEffect::WeakensControl),
        ("요구", SafetyControlActionEffect::EnforcesControl),
        ("유지", SafetyControlActionEffect::EnforcesControl),
        ("보존", SafetyControlActionEffect::EnforcesControl),
        ("활성화", SafetyControlActionEffect::EnforcesControl),
        ("빼", SafetyControlActionEffect::WeakensControl),
        ("끄", SafetyControlActionEffect::WeakensControl),
        ("꺼", SafetyControlActionEffect::WeakensControl),
        ("켜", SafetyControlActionEffect::EnforcesControl),
    ]
}

fn korean_action_negated(words: &[&str], stem: &str, class: KoreanActionClass) -> bool {
    let Some(suffix) = words.first().and_then(|word| word.strip_prefix(stem)) else {
        return false;
    };
    let suffix = if class == KoreanActionClass::Ha {
        suffix.strip_prefix("하").unwrap_or(suffix)
    } else {
        suffix
    };
    korean_closed_negative_suffix(suffix, &words[1..])
}

fn korean_closed_negative_suffix(suffix: &str, remainder: &[&str]) -> bool {
    (matches!(suffix, "지마" | "지마세요" | "지않아" | "지않고") && remainder.is_empty())
        || (suffix == "지"
            && matches!(
                remainder,
                ["마"]
                    | ["마세요"]
                    | ["말고"]
                    | ["않아"]
                    | ["않고"]
                    | ["않게", "해"]
                    | ["않도록", "설정해"]
            ))
        || (suffix == "면"
            && matches!(
                remainder,
                ["안", "돼"] | ["안", "돼요"] | ["안", "됩니다"] | ["안", "됨"]
            ))
}

fn korean_forced_action(words: &[&str]) -> bool {
    let Some(stem) = words.first().and_then(|word| korean_action_stem(word)) else {
        return false;
    };
    let Some(suffix) = words.first().and_then(|word| word.strip_prefix(stem)) else {
        return false;
    };
    let suffix = suffix.strip_prefix("하").unwrap_or(suffix);
    suffix == "지"
        && matches!(
            &words[1..],
            ["않으면", "안", "돼"] | ["않으면", "안", "돼요"] | ["않으면", "안", "됩니다"]
        )
}

fn korean_ha_command(word: &str, stem: &str) -> bool {
    word.strip_prefix(stem).is_some_and(|suffix| {
        matches!(
            suffix,
            "" | "해" | "해줘" | "해주세요" | "하세요" | "해야해" | "해야합니다" | "한다"
        )
    })
}

fn korean_control_target_prefix_length(words: &[&str]) -> Option<usize> {
    let mut index = 0usize;
    loop {
        let (length, coordinated) = korean_control_target_at(&words[index..])?;
        index = index.saturating_add(length);
        if !coordinated {
            break;
        }
        if index >= words.len() {
            return None;
        }
    }
    Some(index)
}

fn korean_control_target_at(words: &[&str]) -> Option<(usize, bool)> {
    for (first, second) in [
        ("사용자", "승인"),
        ("안전", "게이트"),
        ("안전", "장치"),
        ("보호", "장치"),
    ] {
        if words.first() == Some(&first) {
            let particle = korean_target_particle(words.get(1)?, second)?;
            return Some((2, matches!(particle, "과" | "와")));
        }
    }
    for target in [
        "안전게이트",
        "안전장치",
        "보호장치",
        "미리보기",
        "검증",
        "승인",
    ] {
        if let Some(particle) = words
            .first()
            .and_then(|word| korean_target_particle(word, target))
        {
            return Some((1, matches!(particle, "과" | "와")));
        }
    }
    None
}

fn korean_target_particle<'a>(word: &'a str, target: &str) -> Option<&'a str> {
    word.strip_prefix(target).filter(|suffix| {
        matches!(
            *suffix,
            "" | "을" | "를" | "이" | "가" | "은" | "는" | "만" | "도" | "과" | "와"
        )
    })
}

fn korean_stem_with_particle(word: &str, stems: &[&str]) -> bool {
    stems.iter().any(|stem| {
        word.strip_prefix(stem)
            .is_some_and(|suffix| matches!(suffix, "" | "을" | "를"))
    })
}
