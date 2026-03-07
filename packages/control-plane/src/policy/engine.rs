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

pub fn can_hire_second_ceo(_actor_role: Role) -> Decision {
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

/// Context for evaluating file transfer permissions.
/// The route handler populates this from the two Agent records.
#[derive(Debug, Clone)]
pub struct FileTransferContext {
    pub sender_role: Role,
    pub receiver_role: Role,
    pub sender_id: uuid::Uuid,
    pub receiver_id: uuid::Uuid,
    pub sender_parent: Option<uuid::Uuid>,
    pub receiver_parent: Option<uuid::Uuid>,
    pub sender_company: Option<uuid::Uuid>,
    pub receiver_company: Option<uuid::Uuid>,
}

/// Evaluates whether a file transfer is permitted under the hierarchy rules.
///
/// Allowed transfers:
///   - WORKER ↔ peer WORKER (same parent_agent_id, same company)
///   - WORKER → their MANAGER (sender.parent = receiver.id)
///   - MANAGER → their WORKER (receiver.parent = sender.id)
///   - MANAGER ↔ peer MANAGER (same company_id)
///   - MANAGER → their CEO (sender.parent = receiver.id)
///   - CEO → their MANAGER (receiver.parent = sender.id)
///   - CEO → MAIN (always)
///   - MAIN → any CEO (always)
///   - MAIN → any MANAGER or WORKER (full downward authority)
///
/// Denied:
///   - CEO → CEO (cross-company must route through MAIN)
///   - WORKER → CEO or MAIN directly (must go through manager)
///   - CEO → WORKER directly (must go through manager)
pub fn can_send_file(ctx: &FileTransferContext) -> Decision {
    use Role::*;

    match (&ctx.sender_role, &ctx.receiver_role) {
        // Workers in same department (same parent, same company)
        (Worker, Worker) => {
            let same_parent = ctx.sender_parent.is_some()
                && ctx.sender_parent == ctx.receiver_parent;
            let same_company = ctx.sender_company.is_some()
                && ctx.sender_company == ctx.receiver_company;
            if same_parent && same_company {
                Decision::AllowedImmediate
            } else {
                Decision::Denied(
                    "Workers can only send files to peers in the same department (same manager)".into()
                )
            }
        }

        // Worker → their Manager (upward)
        (Worker, Manager) => {
            if ctx.sender_parent == Some(ctx.receiver_id) {
                Decision::AllowedImmediate
            } else {
                Decision::Denied("Workers can only send files to their own manager".into())
            }
        }

        // Manager → their Worker (downward)
        (Manager, Worker) => {
            if ctx.receiver_parent == Some(ctx.sender_id) {
                Decision::AllowedImmediate
            } else {
                Decision::Denied("Managers can only send files to their own workers".into())
            }
        }

        // Manager ↔ peer Manager (same company)
        (Manager, Manager) => {
            let same_company = ctx.sender_company.is_some()
                && ctx.sender_company == ctx.receiver_company;
            if same_company {
                Decision::AllowedImmediate
            } else {
                Decision::Denied(
                    "Managers can only send files to other managers in the same company".into()
                )
            }
        }

        // Manager → their CEO (upward)
        (Manager, Ceo) => {
            if ctx.sender_parent == Some(ctx.receiver_id) {
                Decision::AllowedImmediate
            } else {
                Decision::Denied("Managers can only send files to their own CEO".into())
            }
        }

        // CEO → their Manager (downward)
        (Ceo, Manager) => {
            if ctx.receiver_parent == Some(ctx.sender_id) {
                Decision::AllowedImmediate
            } else {
                Decision::Denied("CEOs can only send files to their own managers".into())
            }
        }

        // CEO → MAIN (always allowed)
        (Ceo, Main) => Decision::AllowedImmediate,

        // MAIN → any CEO (always allowed)
        (Main, Ceo) => Decision::AllowedImmediate,

        // MAIN → any Manager or Worker (full downward authority)
        (Main, Manager) | (Main, Worker) => Decision::AllowedImmediate,

        // CEO → CEO: denied (cross-company must route through MAIN)
        (Ceo, Ceo) => Decision::Denied(
            "CEOs cannot send files directly to other CEOs. \
             Send to MAIN (KonnerBot) who can forward to any CEO.".into()
        ),

        // Worker → CEO/MAIN directly: denied
        (Worker, Ceo) | (Worker, Main) => Decision::Denied(
            "Workers must send files through their manager, not directly to CEO or MAIN".into()
        ),

        // CEO → Worker directly: denied
        (Ceo, Worker) => Decision::Denied(
            "CEOs cannot send files directly to workers — send to the worker's manager instead".into()
        ),

        // Everything else
        _ => Decision::Denied("File transfer not permitted between these roles".into()),
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
