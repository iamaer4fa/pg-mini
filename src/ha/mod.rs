#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeRole {
    Primary,
    Standby,
}

impl NodeRole {
    pub fn label(self) -> &'static str {
        match self {
            Self::Primary => "Primary",
            Self::Standby => "Standby",
        }
    }

    pub fn from_recovery_state(is_in_recovery: bool) -> Self {
        if is_in_recovery {
            Self::Standby
        } else {
            Self::Primary
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HaNodeStatus {
    pub role: NodeRole,
}

#[derive(Debug, Clone)]
pub struct HaNode {
    pub name: String,
    pub host_port: String,
    pub database: String,
    pub status: Option<HaNodeStatus>,
    pub error: Option<String>,
    pub is_refreshing: bool,
    pub is_active: bool,
}

#[cfg(test)]
mod tests {
    use super::NodeRole;

    #[test]
    fn maps_postgres_recovery_state_to_node_role() {
        assert_eq!(NodeRole::from_recovery_state(false), NodeRole::Primary);
        assert_eq!(NodeRole::from_recovery_state(true), NodeRole::Standby);
    }
}
