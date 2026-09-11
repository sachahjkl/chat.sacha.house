namespace "staging" {
  capabilities = ["list-jobs", "read-job"]
}

namespace "production" {
  capabilities = ["list-jobs", "parse-job", "read-job", "submit-job"]
}

host_volume "chat-sacha-house-production-data" {
  policy = "write"
}
