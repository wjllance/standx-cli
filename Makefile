CARGO ?= cargo
SYMBOL ?= BTC-USD
MAKER_CONFIG ?= examples/maker.toml
MAKER_ARGS ?=

.PHONY: help maker

help:
	@printf '%s\n' \
		'make maker                         Start the paper maker for BTC-USD' \
		'make maker SYMBOL=ETH-USD          Select a symbol' \
		'make maker MAKER_CONFIG=path.toml  Select a strategy config file' \
		'make maker MAKER_ARGS="--interval 3"  Pass additional maker arguments'

# Paper mode is deliberate: this target never adds --live.
maker:
	$(CARGO) run -p standx-cli -- maker run $(SYMBOL) --maker-config $(MAKER_CONFIG) $(MAKER_ARGS)
