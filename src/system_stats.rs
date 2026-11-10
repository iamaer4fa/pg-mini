use crate::models::QueryResult;

#[derive(Debug)]
pub struct SystemStatsSnapshot {
    pub extension_schema: Option<String>,
    pub sections: Vec<SystemStatsSection>,
}

#[derive(Debug)]
pub struct SystemStatsSection {
    pub title: &'static str,
    pub function_name: &'static str,
    pub result: Result<QueryResult, String>,
    pub is_truncated: bool,
}

pub const SYSTEM_STATS_FUNCTIONS: [(&str, &str); 10] = [
    ("Operating system", "pg_sys_os_info"),
    ("CPU information", "pg_sys_cpu_info"),
    ("CPU usage", "pg_sys_cpu_usage_info"),
    ("Memory", "pg_sys_memory_info"),
    ("Disk I/O", "pg_sys_io_analysis_info"),
    ("Disks", "pg_sys_disk_info"),
    ("Load average", "pg_sys_load_avg_info"),
    ("Processes", "pg_sys_process_info"),
    ("Network", "pg_sys_network_info"),
    ("Process CPU and memory", "pg_sys_cpu_memory_by_process"),
];
