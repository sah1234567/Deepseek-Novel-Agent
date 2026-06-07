#[test]
fn parse_agent_type_accepts_general_purpose() {
    assert_eq!(
        novel_core::AgentType::parse("GeneralPurpose"),
        Some(novel_core::AgentType::GeneralPurpose)
    );
    assert_eq!(
        novel_core::AgentType::parse("general-purpose"),
        Some(novel_core::AgentType::GeneralPurpose)
    );
}
