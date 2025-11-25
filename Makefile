.PHONY: dev release frontend

frontend:
	cd front && bun run build

release: frontend
	cargo build --release

dev: frontend
	cargo run

