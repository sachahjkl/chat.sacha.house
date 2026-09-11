#!/usr/bin/env bash

set -Eeuo pipefail
umask 077

if (( EUID != 0 )); then
  printf 'restore must run as root\n' >&2
  exit 1
fi
if (( $# < 2 || $# > 3 )); then
  printf 'usage: %s {staging|production} BACKUP_FILE.sqlite [BACKUP_FILE.sha256]\n' "$0" >&2
  exit 2
fi
if [[ "$1" != staging && "$1" != production ]]; then
  printf 'namespace must be staging or production\n' >&2
  exit 2
fi

namespace="$1"
archive="$(realpath -- "$2")"
checksum="$(realpath -- "${3:-$2.sha256}")"
volume_name="chat-sacha-house-${namespace}-data"

volume_path="$(nomad volume status -namespace "$namespace" -json "$volume_name" | jq -r .HostPath)"
case "$volume_path" in
  /data/Services/nomad/volumes/*) ;;
  *)
    printf 'Nomad returned an unexpected volume path\n' >&2
    exit 1
    ;;
esac

read -r expected_hash expected_name <"$checksum"
if [[ ! "$expected_hash" =~ ^[0-9a-f]{64}$ ]] || [[ "$expected_name" != "$(basename -- "$archive")" ]]; then
  printf 'backup checksum has an invalid format\n' >&2
  exit 1
fi
actual_hash="$(sha256sum "$archive" | cut -d' ' -f1)"
if [[ "$actual_hash" != "$expected_hash" ]]; then
  printf 'backup checksum verification failed\n' >&2
  exit 1
fi
if [[ "$(sqlite3 "$archive" 'pragma integrity_check;')" != ok ]]; then
  printf 'backup database integrity check failed\n' >&2
  exit 1
fi
job_status="$(nomad job status -namespace "$namespace" -json chat-sacha-house 2>/dev/null | jq -r '.[0].Status' || true)"
if [[ -n "$job_status" && "$job_status" != dead ]]; then
  printf 'stop the %s chat-sacha-house Nomad job before restoration\n' "$namespace" >&2
  exit 1
fi
if fuser "$volume_path/chat.db" >/dev/null 2>&1; then
  printf 'wait for all %s allocations to release the database\n' "$namespace" >&2
  exit 1
fi

rm -f -- "$volume_path/chat.db-wal" "$volume_path/chat.db-shm"
install -o 65532 -g 65532 -m 0600 "$archive" "$volume_path/chat.db"
printf 'restored %s from %s; deploy the validated image to check application health\n' "$volume_name" "$archive"
