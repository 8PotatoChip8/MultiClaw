use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    Main,
    Ceo,
    Manager,
    Worker,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApproverType {
    Ceo,
    MainAgent,
    User,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    AllowedImmediate,
    RequiresRequest {
        request_type: String,
        approver_chain: Vec<ApproverType>,
    },
    Denied(String),
}

/// Evaluates if a CEO can hire a new manager, given the resulting count of managers.
pub fn can_hire_manager(requested_new_manager_count: u32, actor_role: Role) -> Decision {
    if actor_role != Role::Ceo {
        return Decision::Denied("Only CEOs can hire managers".into());
    }

    match requested_new_manager_count {
        1 | 2 => Decision::AllowedImmediate,
        3 => Decision::RequiresRequest {
            request_type: "INCREASE_MANAGER_LIMIT".into(),
            approver_chain: vec![ApproverType::MainAgent],
        },
        _ => Decision::RequiresRequest {
            request_type: "INCREASE_MANAGER_LIMIT".into(),
            approver_chain: vec![ApproverType::MainAgent, ApproverType::User],
        },
    }
}

/// Evaluates if a Manager can hire a new worker, given the resulting count of workers.
pub fn can_hire_worker(requested_new_worker_count: u32, actor_role: Role) -> Decision {
    if actor_role != Role::Manager {
        // Technically CEOs could hire workers but the spec scopes this specifically to managers hiring workers.
        return Decision::Denied("Only Managers can hire workers in this flow".into());
    }

    match requested_new_worker_count {
        1 | 2 | 3 => Decision::AllowedImmediate,
        4 => Decision::RequiresRequest {
            request_type: "INCREASE_WORKER_LIMIT".into(),
            approver_chain: vec![ApproverType::Ceo],
        },
        5 => Decision::RequiresRequest {
            request_type: "INCREASE_WORKER_LIMIT".into(),
            approver_chain: vec![ApproverType::Ceo, ApproverType::MainAgent],
        },
        _ => Decision::RequiresRequest {
            request_type: "INCREASE_WORKER_LIMIT".into(),
            approver_chain: vec![ApproverType::Ceo, ApproverType::MainAgent, ApproverType::User],
        },
    }
}

pub fn can_hire_second_ceo(actor_role: Role) -> Decision {
    // Only user or main agent can init this, but ultimately requires USER.
    Decision::RequiresRequest {
        request_type: "ADD_SECOND_CEO".into(),
        approver_chain: vec![ApproverType::User],
    }
}

pub fn can_start_cross_company_chat(is_engagement_thread: bool) -> Decision {
    if is_engagement_thread {
        Decision::AllowedImmediate
    } else {
        Decision::RequiresRequest {
            request_type: "CROSS_COMPANY_CHAT".into(),
            approver_chain: vec![ApproverType::Ceo],
        }
    }
}

pub fn can_hire_service(actor_role: Role) -> Decision {
    match actor_role {
        Role::Ceo => Decision::AllowedImmediate,
        Role::Manager => Decision::RequiresRequest {
            request_type: "SERVICE_ENGAGEMENT".into(),
            approver_chain: vec![ApproverType::Ceo],
        },
        _ => Decision::Denied("Only CEOs or Managers can hire internal services".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manager_hiring_escalation() {
        assert_eq!(can_hire_manager(2, Role::Ceo), Decision::AllowedImmediate);
        assert_eq!(
            can_hire_manager(3, Role::Ceo),
            Decision::RequiresRequest {
                request_type: "INCREASE_MANAGER_LIMIT".into(),
                approver_chain: vec![ApproverType::MainAgent]
            }
        );
        assert_eq!(
            can_hire_manager(5, Role::Ceo),
            Decision::RequiresRequest {
                request_type: "INCREASE_MANAGER_LIMIT".into(),
                approver_chain: vec![ApproverType::MainAgent, ApproverType::User]
            }
        );
        
        // Non-CEO trying to hire manager
        assert!(matches!(can_hire_manager(1, Role::Manager), Decision::Denied(_)));
    }

    #[test]
    fn test_worker_hiring_escalation() {
        assert_eq!(can_hire_worker(3, Role::Manager), Decision::AllowedImmediate);
        assert_eq!(
            can_hire_worker(4, Role::Manager),
            Decision::RequiresRequest {
                request_type: "INCREASE_WORKER_LIMIT".into(),
                approver_chain: vec![ApproverType::Ceo]
            }
        );
        assert_eq!(
            can_hire_worker(5, Role::Manager),
            Decision::RequiresRequest {
                request_type: "INCREASE_WORKER_LIMIT".into(),
                approver_chain: vec![ApproverType::Ceo, ApproverType::MainAgent]
            }
        );
        assert_eq!(
            can_hire_worker(6, Role::Manager),
            Decision::RequiresRequest {
                request_type: "INCREASE_WORKER_LIMIT".into(),
                approver_chain: vec![ApproverType::Ceo, ApproverType::MainAgent, ApproverType::User]
            }
        );
    }
}
