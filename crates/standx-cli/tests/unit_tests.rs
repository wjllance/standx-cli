//! Unit test harness — wires the `unit/` tree into a compiled test target.

mod unit {
    pub mod models {
        mod market_data_test;
        mod position_test;
        mod symbol_info_test;
    }
    pub mod utils {
        mod error_test;
        mod time_parser_test;
    }
}
