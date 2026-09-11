#!/usr/bin/env bash

set -Eeuo pipefail
umask 077

if (( EUID != 0 )); then
  printf 'backup must run as root\n' >&2
  exit 1
fi
if (( $# != 2 )); then
  printf 'usage: %s {staging|production} BACKUP_FILE.sqlite\n' "$0" >&2
  exit 2
fi
if [[ "$1" != staging && "$1" != production ]]; then
  printf 'namespace must be staging or production\n' >&2
  exit 2
fi

namespace="$1"
backup_file="$(realpath -m -- "$2")"
volume_name="chat-sacha-house-${namespace}-data"
volume_path="$(nomad volume status -namespace "$namespace" -json "$volume_name" | jq -r .HostPath)"
case "$volume_path" in
  /data/Services/nomad/volumes/*) ;;
  *)
    printf 'Nomad returned an unexpected volume path\n' >&2
    exit 1
    ;;
esac

temporary="$(mktemp --tmpdir="$(dirname -- "$backup_file")" .chat-backup.XXXXXXXX)"
trap 'rm -f -- "$temporary"' EXIT
sqlite3 "$volume_path/chat.db" ".backup '$temporary'"
test "$(sqlite3 "$temporary" 'pragma integrity_check;')" = ok
install -o root -g root -m 0600 "$temporary" "$backup_file"
printf '%s  %s\n' "$(sha256sum "$backup_file" | cut -d' ' -f1)" "$(basename -- "$backup_file")" >"${backup_file}.sha256"
chmod 0600 "${backup_file}.sha256"
printf 'backed up %s to %s\n' "$volume_name" "$backup_file"
