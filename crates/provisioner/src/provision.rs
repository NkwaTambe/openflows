use anyhow::Context;
use std::path::PathBuf;
use tracing::{info, warn};

use crate::transport::WorkspaceTransport;

/// Provisions configuration files and skills into worker workspaces.
pub struct Provisioner {
    /// Orchestrator source directory (contains orchestration/).
    orchestrator_dir: PathBuf,
}

impl Provisioner {
    pub fn new(orchestrator_dir: impl Into<PathBuf>) -> Self {
        Self {
            orchestrator_dir: orchestrator_dir.into(),
        }
    }

    /// Materialize all provisioning artifacts into a workspace via the transport.
    ///
    /// Reads `registry.json` for the given role and provisions:
    /// 1. `.agents/skills/<name>/SKILL.md` for each listed skill
    /// 2. `.mcp.json` from the role's mcp config
    /// 3. Standards files (CODING.md, SECURITY.md, REVIEW.md)
    /// 4. Role persona file (as `<role>.agent.md` and `AGENTS.md`)
    pub async fn provision_role(
        &self,
        transport: &dyn WorkspaceTransport,
        role: &str,
        registry: &config::Registry,
    ) -> anyhow::Result<()> {
        let entry = registry
            .get(role)
            .ok_or_else(|| anyhow::anyhow!("Role '{}' not found in registry", role))?;

        if !entry.enabled {
            info!(role, "Role is disabled — skipping provisioning");
            return Ok(());
        }

        // 1. Provision skills
        // Try to create .agents/skills directory first, proceed without skills if permission denied.
        // This allows role provisioning to continue even if workspace lacks home directory write access.
        let skills_dir = ".agents/skills";

        if let Err(e) = transport.create_dir_all(skills_dir).await {
            // Permission denied is common in sandboxed workspaces - log and continue without skills
            if e.to_string().contains("Permission denied") || e.to_string().contains("mkdir") {
                warn!(role, skills_dir, error = %e, "Cannot create .agents/skills directory - skills will not be provisioned. Ensure workspace has write access to home directory.");
            } else {
                warn!(role, skills_dir, error = %e, "Failed to create skills directory - continuing without skills");
            }
        } else {
            // Successfully created directory, now provision each skill
            for skill_name in &entry.skills {
                let skill_dir = self
                    .orchestrator_dir
                    .join("orchestration")
                    .join("plugin")
                    .join("skills")
                    .join(skill_name);

                let skill_md = skill_dir.join("SKILL.md");
                if skill_md.exists() {
                    let target = format!("{}/{}/SKILL.md", skills_dir, skill_name);
                    match transport.copy_file(&skill_md, &target).await {
                        Ok(_) => info!(skill = skill_name, role, "Provisioned skill"),
                        Err(e) => {
                            warn!(skill = skill_name, role, error = %e, "Failed to copy skill file - continuing");
                        }
                    }
                } else {
                    warn!(skill = skill_name, "Skill directory not found — skipping");
                }
            }
        }

        // 2. Provision .mcp.json
        if !entry.mcp.is_null() && !entry.mcp.as_object().map(|o| o.is_empty()).unwrap_or(true) {
            let mcp_json = serde_json::to_string_pretty(&entry.mcp)?;
            transport
                .write_file(".mcp.json", &mcp_json)
                .await
                .context("Failed to write .mcp.json")?;
            info!(role, "Provisioned .mcp.json");
        }

        // 3. Provision standards files
        let standards_dir = self
            .orchestrator_dir
            .join("orchestration")
            .join("agent")
            .join("standards");

        for standard in &["CODING.md", "SECURITY.md", "REVIEW.md"] {
            let path = standards_dir.join(standard);
            if path.exists() {
                transport
                    .copy_file(&path, standard)
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to provision {}: {}", standard, e))?;
                info!(standard, role, "Provisioned standard");
            }
        }

        // 4. Provision role persona
        let persona_path = self
            .orchestrator_dir
            .join("orchestration")
            .join("agent")
            .join("agents")
            .join(format!("{}.agent.md", role));

        if persona_path.exists() {
            transport
                .copy_file(&persona_path, &format!("{}.agent.md", role))
                .await
                .map_err(|e| anyhow::anyhow!("Failed to provision persona: {}", e))?;
            info!(role, "Provisioned persona");

            // Also materialize the persona as the workspace's AGENTS.md. Coder's
            // Coder Agents reads AGENTS.md from the agent's working directory (and
            // ~/.coder/AGENTS.md) and injects it into the system prompt for every
            // conversation in this workspace, so the persona is delivered
            // server-side and persists across chats instead of being bundled into a
            // fragile first request.
            transport
                .copy_file(&persona_path, "AGENTS.md")
                .await
                .map_err(|e| anyhow::anyhow!("Failed to provision AGENTS.md persona: {}", e))?;
            info!(role, "Provisioned AGENTS.md persona");
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use config::registry::{Registry, RegistryEntry};
    use std::collections::HashMap;
    use std::path::Path;

    /// In-memory transport that records all file writes for assertions.
    #[derive(Default)]
    struct MemTransport {
        files: std::sync::Mutex<HashMap<String, String>>,
    }

    impl MemTransport {
        fn written(&self, path: &str) -> Option<String> {
            self.files.lock().unwrap().get(path).cloned()
        }
    }

    #[async_trait]
    impl WorkspaceTransport for MemTransport {
        async fn read_file(&self, path: &str) -> anyhow::Result<String> {
            self.files
                .lock()
                .unwrap()
                .get(path)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("file not found: {}", path))
        }
        async fn write_file(&self, path: &str, content: &str) -> anyhow::Result<()> {
            self.files
                .lock()
                .unwrap()
                .insert(path.to_string(), content.to_string());
            Ok(())
        }
        async fn execute(&self, _command: &str) -> anyhow::Result<crate::transport::CommandOutput> {
            Ok(crate::transport::CommandOutput {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 0,
            })
        }
        async fn list_directory(
            &self,
            _path: &str,
        ) -> anyhow::Result<Vec<crate::transport::DirEntry>> {
            Ok(vec![])
        }
        async fn symlink_or_copy(&self, _source: &Path, _target: &str) -> anyhow::Result<()> {
            Ok(())
        }
        async fn create_dir_all(&self, _path: &str) -> anyhow::Result<()> {
            Ok(())
        }
        async fn path_exists(&self, path: &str) -> bool {
            self.files.lock().unwrap().contains_key(path)
        }
        async fn remove_dir_all(&self, _path: &str) -> anyhow::Result<()> {
            Ok(())
        }
        async fn copy_file(&self, source_local: &Path, target: &str) -> anyhow::Result<()> {
            let content = std::fs::read_to_string(source_local)?;
            self.write_file(target, &content).await
        }
    }

    fn persona_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let agents = dir.path().join("orchestration/agent/agents");
        std::fs::create_dir_all(&agents).unwrap();
        std::fs::write(
            agents.join("forge.agent.md"),
            "# Forge persona\nBuild awesome code.\n",
        )
        .unwrap();
        dir
    }

    fn registry() -> Registry {
        Registry {
            default_cli: "claude".to_string(),
            allowed_domains: vec![],
            team: vec![
                RegistryEntry {
                    id: "forge".to_string(),
                    enabled: true,
                    plan_mode: false,
                    max_instances: 1,
                    skills: vec![],
                    mcp: serde_json::Value::Null,
                    cli: String::new(),
                    active: true,
                    instances: 1,
                    model_backend: None,
                    routing_key: None,
                    github_token_env: None,
                    allowed_domains: None,
                    coder_module: None,
                    model: None,
                },
                RegistryEntry {
                    id: "lore".to_string(),
                    enabled: false,
                    plan_mode: false,
                    max_instances: 1,
                    skills: vec![],
                    mcp: serde_json::Value::Null,
                    cli: String::new(),
                    active: true,
                    instances: 1,
                    model_backend: None,
                    routing_key: None,
                    github_token_env: None,
                    allowed_domains: None,
                    coder_module: None,
                    model: None,
                },
            ],
        }
    }

    #[tokio::test]
    async fn provisions_agents_md_persona_for_enabled_role() {
        let orch = persona_dir();
        let transport = MemTransport::default();
        let provisioner = Provisioner::new(orch.path());
        provisioner
            .provision_role(&transport, "forge", &registry())
            .await
            .unwrap();

        let agents_md = transport.written("AGENTS.md").expect("AGENTS.md written");
        assert!(agents_md.contains("Forge persona"));
        let persona = transport
            .written("forge.agent.md")
            .expect("forge.agent.md written");
        assert!(persona.contains("Forge persona"));
    }

    #[tokio::test]
    async fn skips_agents_md_for_disabled_role() {
        let orch = persona_dir();
        let transport = MemTransport::default();
        let provisioner = Provisioner::new(orch.path());
        provisioner
            .provision_role(&transport, "lore", &registry())
            .await
            .unwrap();

        assert_eq!(transport.written("AGENTS.md"), None);
        assert_eq!(transport.written("lore.agent.md"), None);
    }
}
