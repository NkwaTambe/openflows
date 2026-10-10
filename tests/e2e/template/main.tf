terraform {
  required_providers {
    coder  = { source = "coder/coder", version = "2.18.0" }
    docker = { source = "kreuzwerker/docker", version = "4.5.0" }
  }
}

variable "dev_binary_host_path" {
  # Reuse the client's existing template-variable transport to pass the unique
  # compose project name, which identifies this run's image and network.
  type = string
}

data "coder_workspace" "me" {}
data "coder_parameter" "tenant" {
  name    = "tenant"
  type    = "string"
  default = "ci"
}

resource "coder_agent" "main" {
  os   = "linux"
  arch = "amd64"
  dir  = "/home/coder/workspace"
  env = {
    REDIS_URL        = "redis://redis:6379"
    OPENFLOWS_TENANT = data.coder_parameter.tenant.value
    OPENFLOWS_TICKET = "T-1"
    OPENFLOWS_ROLE   = "forge"
  }
  startup_script = <<-EOT
    #!/bin/bash
    set -euo pipefail
    cd /home/coder/workspace
    git init
    git config user.email ci@example.test
    git config user.name CI
    git commit --allow-empty -m 'CI seed'
    printf '# CI plan\n\nExercise real worker coordination.\n' > /tmp/plan.md
    printf 'CI review evidence\n' > /tmp/review.md
    openflows-harness --help >/dev/null
  EOT
}

resource "docker_container" "worker" {
  count = data.coder_workspace.me.start_count
  name  = "${var.dev_binary_host_path}-worker-${data.coder_workspace.me.id}"
  image = "${var.dev_binary_host_path}-worker:ci"
  labels {
    label = "openflows.ci.project"
    value = var.dev_binary_host_path
  }
  networks_advanced {
    name = "${var.dev_binary_host_path}_default"
  }
  env = [
    "CODER_AGENT_TOKEN=${coder_agent.main.token}",
    "CODER_AGENT_URL=http://coder:7080",
  ]
  entrypoint = ["sh", "-c", coder_agent.main.init_script]
}
