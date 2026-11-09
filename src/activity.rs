#[derive(Debug, Clone)]
pub struct ActivitySession {
    pub pid: i32,
    pub username: String,
    pub database: String,
    pub application: String,
    pub client_addr: String,
    pub state: String,
    pub wait_event: String,
    pub duration_seconds: Option<f64>,
    pub query: String,
}

#[derive(Debug, Clone)]
pub struct ActivityLock {
    pub blocked_pid: i32,
    pub blocked_user: String,
    pub database: String,
    pub wait_seconds: Option<f64>,
    pub blocking_pid: i32,
    pub blocking_user: String,
    pub blocked_query: String,
    pub blocking_query: String,
}

#[derive(Debug, Clone)]
pub struct SizeEntry {
    pub name: String,
    pub size: String,
    pub bytes: i64,
}

#[derive(Debug, Clone, Default)]
pub struct ActivitySnapshot {
    pub sessions: Vec<ActivitySession>,
    pub locks: Vec<ActivityLock>,
    pub database_sizes: Vec<SizeEntry>,
    pub table_sizes: Vec<SizeEntry>,
}
