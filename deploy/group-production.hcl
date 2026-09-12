    task "backup" {
      lifecycle {
        hook    = "prestart"
        sidecar = false
      }

      driver = "docker"

      config {
        image        = [[ var "image" . | quote ]]
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
