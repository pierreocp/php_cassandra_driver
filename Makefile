EXTENSION_NAME := php_cassandra_driver
EXTENSION_SO := target/release/lib$(EXTENSION_NAME).so
PHP_CONFIG ?= php-config
PHP ?= php
DOCKER ?= docker

.PHONY: build print-so install run-example docker-build-so docker-run clean

build:
	cargo build --release
	@echo "Built $(EXTENSION_SO)"

print-so:
	@echo "$(EXTENSION_SO)"

install: build
	install_dir="$$($(PHP_CONFIG) --extension-dir)"; \
	cp "$(EXTENSION_SO)" "$$install_dir/$(EXTENSION_NAME).so"; \
	echo "Copied to $$install_dir/$(EXTENSION_NAME).so"; \
	echo "Enable it with: echo 'extension=$(EXTENSION_NAME).so' > your PHP conf.d file"

run-example: build
	$(PHP) -d extension=$(EXTENSION_SO) examples/test.php

docker-build-so:
	$(DOCKER) build --target artifact --output type=local,dest=dist .
	@echo "Built dist/$(EXTENSION_NAME).so"

docker-run:
	$(DOCKER) compose up --build php

clean:
	cargo clean
	rm -rf dist
