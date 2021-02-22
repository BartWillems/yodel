SHELL := /bin/sh

.ONESHELL:
frontend:
	cd frontend
	yarn
	yarn build

server:
	cargo build --release

all: frontend server

install:
	cp target/release/yodel /opt/yodel/bin/yodel
	cp -r frontend/build /opt/yodel/frontend

clean:
	cargo clean
	rm -rf frontend/node_modules

.PHONY: \
	server \
	frontend \
	build