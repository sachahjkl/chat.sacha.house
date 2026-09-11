variable "image" {
  type        = string
  description = "Immutable GHCR image reference"

  validation {
    condition     = strlen(var.image) == 106 && substr(var.image, 0, 42) == "ghcr.io/sachahjkl/chat.sacha.house@sha256:"
    error_message = "The image must use the Chat GHCR repository and an exact SHA-256 digest."
  }
}

job "chat-sacha-house" {
  namespace   = "production"
  datacenters = ["homelab"]
  type        = "service"

  meta {
    image = var.image
  }

  group "web" {
    count = 1

    update {
      max_parallel      = 1
      health_check      = "checks"
      min_healthy_time  = "10s"
      healthy_deadline  = "2m"
      progress_deadline = "5m"
      auto_revert       = true
    }

    restart {
      attempts = 3
      interval = "10m"
      delay    = "15s"
      mode     = "fail"
    }

    reschedule {
      attempts       = 3
      interval       = "1h"
      delay          = "30s"
      delay_function = "exponential"
      max_delay      = "5m"
      unlimited      = false
    }

    network {
      mode = "host"

      port "http" {
        static       = 9102
        to           = 3030
        host_network = "loopback"
      }
    }

    volume "data" {
      type            = "host"
      source          = "chat-sacha-house-production-data"
      attachment_mode = "file-system"
      access_mode     = "single-node-writer"
      sticky          = true
    }

    task "backup" {
      lifecycle {
        hook    = "prestart"
        sidecar = false
      }

      driver = "docker"

      config {
        image        = var.image
        command      = "/bin/sh"
        args         = ["-ec", "test -s /data/chat.db; mkdir -p /data/backups; archive=/data/backups/pre-deploy-$NOMAD_ALLOC_ID.sqlite; sqlite3 /data/chat.db \".backup '$archive'\"; test -s $archive; gzip $archive"]
        network_mode = "services"
      }

      volume_mount {
        volume      = "data"
        destination = "/data"
      }

      resources {
        cpu    = 100
        memory = 128
      }
    }

    task "web" {
      driver = "docker"

      config {
        image        = var.image
        network_mode = "services"
        ports        = ["http"]
      }

      env {
        BIND_HOST    = "0.0.0.0"
        BIND_PORT    = "3030"
        DATABASE_URL = "sqlite:/data/chat.db?mode=rwc"
      }

      volume_mount {
        volume      = "data"
        destination = "/data"
      }

      service {
        name     = "chat-sacha-house-production"
        provider = "nomad"
        port     = "http"

        check {
          name     = "HTTP health"
          type     = "http"
          path     = "/api/health"
          interval = "10s"
          timeout  = "2s"

          check_restart {
            limit           = 3
            grace           = "30s"
            ignore_warnings = false
          }
        }
      }

      resources {
        cpu    = 500
        memory = 512
      }

      logs {
        max_files     = 5
        max_file_size = 10
      }

      kill_timeout = "30s"
    }
  }
}
