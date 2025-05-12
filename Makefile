RUSTFLAGS := -A dead_code -A unused_imports -A non_snake_case -A unused_variables -A unreachable_code

build:
	RUSTFLAGS="$(RUSTFLAGS)" cargo build --bin ptp
	RUSTFLAGS="$(RUSTFLAGS)" cargo build --bin mpcn

runt:
	RUSTFLAGS="$(RUSTFLAGS)" cargo run --bin ptp -- $(ARGS)

runn:
	RUSTFLAGS="$(RUSTFLAGS)" cargo run --bin mpcn
