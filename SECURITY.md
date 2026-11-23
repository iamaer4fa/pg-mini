# Security Policy

## Supported Versions

pg-mini is currently in **Beta**. Because the project is evolving rapidly and is pre-1.0.0, security patches are only provided for the latest release.

| Version | Supported          |
| ------- | ------------------ |
| 0.1.15   | :white_check_mark: |
| 0.1.14   | :white_check_mark: |
| < 0.1.0   | :x:                |


If you are running an older version, please upgrade to the latest release before reporting a security issue.

# Reporting a Vulnerability
**Please do not report security vulnerabilities through public GitHub issues.**

Because pg-mini is a database client that handles connection credentials, TLS certificates, and potentially sensitive SQL queries, I take security seriously. If you discover a vulnerability, please report it privately so I can address it before it is publicly disclosed.

# How to Report

Please use **GitHub's Private Vulnerability Reporting** feature:

1. Go to the Security tab of this repository.
2. Click "Report a vulnerability" on the left sidebar.
3. Fill out the form with as much detail as possible.

If you are unable to use that feature, you can contact me directly at allan.registos [@] gmail.com 

# What to Include

To help me triage and fix the issue quickly, please include:
* A description of the vulnerability and its potential impact.
* Steps to reproduce the issue (proof-of-concept code, screenshots, or a video if applicable).
* The version of pg-mini you are using, and your OS (Linux distro, Windows, or macOS).
* Any suggested mitigations or fixes.

# What to Expect

As a solo maintainer, I will do my best to respond promptly:

* **Acknowledgment**: I will acknowledge receipt of your report within 48–72 hours.
* **Updates**: I will provide an update on the status of the vulnerability within 7 days.
* **Resolution**: If accepted, I will work on a fix and aim to release a patched version as soon as possible. I will credit you in the release notes (unless you prefer to remain anonymous).
* **Declined Reports**: If the issue is determined not to be a security vulnerability (e.g., a bug or a theoretical issue with no practical impact), I will explain my reasoning and, if appropriate, suggest opening a regular public issue.

# Security Scope
Given the nature of pg-mini, the following areas are considered in-scope for security reports:

* **Credential Handling:** How connection profiles, passwords, and TLS certificates are stored (e.g., plaintext password exports, config file permissions).
* **TLS Verification:** Bypasses or flaws in the TLS verification logic (e.g., the "Prefer" mode falling back to plaintext unexpectedly).
* **SQL Injection:** Vulnerabilities where user input in the UI (like table names or filters) could execute unintended SQL.
* **Backup/Restore Execution:** Command injection or path traversal issues when invoking pg_dump, pg_restore, or psql.
* **Dependency Vulnerabilities:** Critical CVEs in the Rust crates used by pg-mini.

Thank you for helping keep pg-mini safe for everyone!
