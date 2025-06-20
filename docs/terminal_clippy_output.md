    = help: to override `-D warnings` add `#[allow(clippy::ptr_arg)]`

error: returning the result of a `let` binding from a block
   --> src/arbitrage/strategy.rs:249:21
    |
248 |                     let rate = rate_decimal.to_f64().unwrap_or(1.0);
    |                     ------------------------------------------------ unnecessary `let` binding
249 |                     rate
    |                     ^^^^
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#let_and_return
help: return the expression directly
    |
248 ~                     
249 ~                     rate_decimal.to_f64().unwrap_or(1.0)
    |

error: returning the result of a `let` binding from a block
   --> src/arbitrage/strategy.rs:261:25
    |
260 |                         let rate = rate_decimal.to_f64().unwrap_or(0.0);
    |                         ------------------------------------------------ unnecessary `let` binding
261 |                         rate
    |                         ^^^^
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#let_and_return
help: return the expression directly
    |
260 ~                         
261 ~                         rate_decimal.to_f64().unwrap_or(0.0)
    |

error: you seem to be trying to use `match` for destructuring a single pattern. Consider using `if let`
   --> src/dex/clients/jupiter.rs:637:9
    |
637 | /         match *circuit_breaker {
638 | |             CircuitBreakerState::HalfOpen => {
639 | |                 // Transition to closed after successful test
640 | |                 *circuit_breaker = CircuitBreakerState::Closed;
...   |
643 | |             _ => {} // Already closed or success in normal operation
644 | |         }
    | |_________^
    |
    = note: you might want to preserve the comments from inside the `match`
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#single_match
    = note: `-D clippy::single-match` implied by `-D warnings`
    = help: to override `-D warnings` add `#[allow(clippy::single_match)]`
help: try
    |
637 ~         if let CircuitBreakerState::HalfOpen = *circuit_breaker {
638 +             // Transition to closed after successful test
639 +             *circuit_breaker = CircuitBreakerState::Closed;
640 ~             debug!("🟢 Jupiter circuit breaker closed after successful test");
641 +         }
    |

error: the borrowed expression implements the required traits
   --> src/dex/clients/jupiter.rs:897:19
    |
897 |             .post(&format!("{}/swap", JUPITER_API_BASE))
    |                   ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ help: change this to: `format!("{}/swap", JUPITER_API_BASE)`
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#needless_borrows_for_generic_args
    = note: `-D clippy::needless-borrows-for-generic-args` implied by `-D warnings`
    = help: to override `-D warnings` add `#[allow(clippy::needless_borrows_for_generic_args)]`

error: useless conversion to the same type: `anyhow::Error`
  --> src/dex/clients/lifinity.rs:99:58
   |
99 |         let token_a_account_data = token_a_account_result.map_err(anyhow::Error::from)?;
   |                                                          ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ help: consider removing
   |
   = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#useless_conversion
   = note: `-D clippy::useless-conversion` implied by `-D warnings`
   = help: to override `-D warnings` add `#[allow(clippy::useless_conversion)]`

error: useless conversion to the same type: `anyhow::Error`
   --> src/dex/clients/lifinity.rs:100:58
    |
100 |         let token_b_account_data = token_b_account_result.map_err(anyhow::Error::from)?;
    |                                                          ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ help: consider removing
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#useless_conversion

error: useless conversion to the same type: `anyhow::Error`
   --> src/dex/clients/lifinity.rs:101:52
    |
101 |         let token_a_mint_data = token_a_mint_result.map_err(anyhow::Error::from)?;
    |                                                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ help: consider removing
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#useless_conversion

error: useless conversion to the same type: `anyhow::Error`
   --> src/dex/clients/lifinity.rs:102:52
    |
102 |         let token_b_mint_data = token_b_mint_result.map_err(anyhow::Error::from)?;
    |                                                    ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ help: consider removing
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#useless_conversion

error: useless use of `format!`
   --> src/dex/clients/lifinity.rs:112:19
    |
112 |             name: format!("Lifinity Pool"),
    |                   ^^^^^^^^^^^^^^^^^^^^^^^^ help: consider using `.to_string()`: `"Lifinity Pool".to_string()`
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#useless_format
    = note: `-D clippy::useless-format` implied by `-D warnings`
    = help: to override `-D warnings` add `#[allow(clippy::useless_format)]`

error: you should consider adding a `Default` implementation for `LifinityClient`
   --> src/dex/clients/lifinity.rs:167:5
    |
167 | /     pub fn new() -> Self {
168 | |         Self {
169 | |             name: "Lifinity".to_string(),
170 | |         }
171 | |     }
    | |_____^
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#new_without_default
help: try adding this
    |
166 + impl Default for LifinityClient {
167 +     fn default() -> Self {
168 +         Self::new()
169 +     }
170 + }
    |

error: you don't need to add `&` to all patterns
   --> src/dex/clients/meteora.rs:100:9
    |
100 | /         match program_id {
101 | |             &METEORA_DYNAMIC_AMM_PROGRAM_ID => {
102 | |                 if data_len >= DYNAMIC_AMM_POOL_STATE_SIZE {
103 | |                     Some(MeteoraPoolType::DynamicAmm)
...   |
115 | |             _ => None,
116 | |         }
    | |_________^
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#match_ref_pats
    = note: `-D clippy::match-ref-pats` implied by `-D warnings`
    = help: to override `-D warnings` add `#[allow(clippy::match_ref_pats)]`
help: instead of prefixing all patterns with `&`, you can dereference the expression
    |
100 ~         match *program_id {
101 ~             METEORA_DYNAMIC_AMM_PROGRAM_ID => {
102 |                 if data_len >= DYNAMIC_AMM_POOL_STATE_SIZE {
...
107 |             }
108 ~             METEORA_DLMM_PROGRAM_ID => {
    |

error: useless use of `format!`
   --> src/dex/clients/meteora.rs:144:19
    |
144 |             name: format!("Meteora Dynamic AMM Pool"),
    |                   ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ help: consider using `.to_string()`: `"Meteora Dynamic AMM Pool".to_string()`
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#useless_format

error: useless use of `format!`
   --> src/dex/clients/meteora.rs:193:19
    |
193 |             name: format!("Meteora DLMM Pool"),
    |                   ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ help: consider using `.to_string()`: `"Meteora DLMM Pool".to_string()`
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#useless_format

error: you should consider adding a `Default` implementation for `MeteoraClient`
   --> src/dex/clients/meteora.rs:351:5
    |
351 | /     pub fn new() -> Self {
352 | |         Self {
353 | |             name: "Meteora".to_string(),
354 | |         }
355 | |     }
    | |_____^
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#new_without_default
help: try adding this
    |
350 + impl Default for MeteoraClient {
351 +     fn default() -> Self {
352 +         Self::new()
353 +     }
354 + }
    |

error: casting to the same type is unnecessary (`u16` -> `u16`)
   --> src/dex/clients/meteora.rs:460:32
    |
460 |                 let bin_step = pool.tick_spacing.unwrap_or(1) as u16;
    |                                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ help: try: `pool.tick_spacing.unwrap_or(1)`
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#unnecessary_cast

error: casting to the same type is unnecessary (`u128` -> `u128`)
   --> src/dex/clients/meteora.rs:461:40
    |
461 |                 let liquidity_in_bin = pool.liquidity.unwrap_or(1_000_000_000_000) as u128;
    |                                        ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ help: try: `pool.liquidity.unwrap_or(1_000_000_000_000)`
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#unnecessary_cast

error: you seem to be trying to use `match` for destructuring a single pattern. Consider using `if let`
   --> src/dex/clients/orca.rs:207:5
    |
207 | /     match value {
208 | |         serde_json::Value::Array(arr) => {
209 | |             for item in arr {
210 | |                 match &item {
...   |
227 | |         _ => {}
228 | |     }
    | |_____^
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#single_match
help: try
    |
207 ~     if let serde_json::Value::Array(arr) = value {
208 +         for item in arr {
209 +             match &item {
210 +                 serde_json::Value::String(addr) => {
211 +                     if let Ok(pk) = Pubkey::from_str(addr) {
212 +                         addresses.push(pk);
213 +                     }
214 +                 }
215 +                 serde_json::Value::Object(map) => {
216 +                     if let Some(serde_json::Value::String(addr)) = map.get("address") {
217 +                         if let Ok(pk) = Pubkey::from_str(addr) {
218 +                             addresses.push(pk);
219 +                         }
220 +                     }
221 +                 }
222 +                 _ => {}
223 +             }
224 +         }
225 +     }
    |

error: useless use of `format!`
   --> src/dex/clients/orca.rs:251:19
    |
251 |             name: format!("Orca Whirlpool"),
    |                   ^^^^^^^^^^^^^^^^^^^^^^^^^ help: consider using `.to_string()`: `"Orca Whirlpool".to_string()`
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#useless_format

error: useless use of `format!`
   --> src/dex/clients/orca.rs:298:19
    |
298 |             name: format!("Orca Whirlpool"),
    |                   ^^^^^^^^^^^^^^^^^^^^^^^^^ help: consider using `.to_string()`: `"Orca Whirlpool".to_string()`
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#useless_format

error: you should consider adding a `Default` implementation for `OrcaClient`
   --> src/dex/clients/orca.rs:341:5
    |
341 | /     pub fn new() -> Self {
342 | |         Self {
343 | |             name: "Orca".to_string(),
344 | |         }
345 | |     }
    | |_____^
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#new_without_default
help: try adding this
    |
340 + impl Default for OrcaClient {
341 +     fn default() -> Self {
342 +         Self::new()
343 +     }
344 + }
    |

error: useless use of `format!`
   --> src/dex/clients/orca.rs:371:31
    |
371 |                         name: format!("Orca Whirlpool"),
    |                               ^^^^^^^^^^^^^^^^^^^^^^^^^ help: consider using `.to_string()`: `"Orca Whirlpool".to_string()`
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#useless_format

error: useless use of `format!`
   --> src/dex/clients/orca.rs:422:31
    |
422 |                         name: format!("Orca Whirlpool"),
    |                               ^^^^^^^^^^^^^^^^^^^^^^^^^ help: consider using `.to_string()`: `"Orca Whirlpool".to_string()`
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#useless_format

error: you should consider adding a `Default` implementation for `PhoenixClient`
   --> src/dex/clients/phoenix.rs:254:5
    |
254 | /     pub fn new() -> Self {
255 | |         Self {
256 | |             name: "Phoenix".to_string(),
257 | |         }
258 | |     }
    | |_____^
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#new_without_default
help: try adding this
    |
252 + impl Default for PhoenixClient {
253 +     fn default() -> Self {
254 +         Self::new()
255 +     }
256 + }
    |

error: useless use of `format!`
   --> src/dex/clients/raydium.rs:134:19
    |
134 |             name: format!("Raydium V4 Pool"),
    |                   ^^^^^^^^^^^^^^^^^^^^^^^^^^ help: consider using `.to_string()`: `"Raydium V4 Pool".to_string()`
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#useless_format

error: useless use of `format!`
   --> src/dex/clients/raydium.rs:187:19
    |
187 |             name: format!("Raydium V4 Pool"),
    |                   ^^^^^^^^^^^^^^^^^^^^^^^^^^ help: consider using `.to_string()`: `"Raydium V4 Pool".to_string()`
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#useless_format

error: you should consider adding a `Default` implementation for `RaydiumClient`
   --> src/dex/clients/raydium.rs:233:5
    |
233 | /     pub fn new() -> Self {
234 | |         Self {
235 | |             name: "Raydium".to_string(),
236 | |         }
237 | |     }
    | |_____^
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#new_without_default
help: try adding this
    |
232 + impl Default for RaydiumClient {
233 +     fn default() -> Self {
234 +         Self::new()
235 +     }
236 + }
    |

error: the borrowed expression implements the required traits
   --> src/dex/discovery.rs:175:29
    |
175 |         writer.write_record(&["token_a", "token_b"])?;
    |                             ^^^^^^^^^^^^^^^^^^^^^^^ help: change this to: `["token_a", "token_b"]`
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#needless_borrows_for_generic_args

error: the borrowed expression implements the required traits
   --> src/dex/discovery.rs:178:33
    |
178 |             writer.write_record(&[&pair.token_a, &pair.token_b])?;
    |                                 ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ help: change this to: `[&pair.token_a, &pair.token_b]`
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#needless_borrows_for_generic_args

error: manual `!RangeInclusive::contains` implementation
   --> src/dex/discovery.rs:446:16
    |
446 |             if ratio < 0.1 || ratio > 10.0 {
    |                ^^^^^^^^^^^^^^^^^^^^^^^^^^^ help: use: `!(0.1..=10.0).contains(&ratio)`
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#manual_range_contains

error: manual `!RangeInclusive::contains` implementation
   --> src/dex/discovery.rs:653:12
    |
653 |         if ratio < 0.1 || ratio > 10.0 {
    |            ^^^^^^^^^^^^^^^^^^^^^^^^^^^ help: use: `!(0.1..=10.0).contains(&ratio)`
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#manual_range_contains

error: manual `!RangeInclusive::contains` implementation
  --> src/dex/math/orca.rs:18:8
   |
18 |     if tick < -443636 || tick > 443636 {
   |        ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ help: use: `!(-443636..=443636).contains(&tick)`
   |
   = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#manual_range_contains

error: clamp-like pattern without using clamp function
  --> src/dex/math/orca.rs:30:8
   |
30 |     Ok(max(MIN_SQRT_PRICE, min(MAX_SQRT_PRICE, sqrt_price_q64)))
   |        ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ help: replace with clamp: `sqrt_price_q64.clamp(MIN_SQRT_PRICE, MAX_SQRT_PRICE)`
   |
   = note: clamp will panic if max < min
   = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#manual_clamp

error: manual `!RangeInclusive::contains` implementation
  --> src/dex/math/orca.rs:59:8
   |
59 |     if sqrt_price < MIN_SQRT_PRICE || sqrt_price > MAX_SQRT_PRICE {
   |        ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ help: use: `!(MIN_SQRT_PRICE..=MAX_SQRT_PRICE).contains(&sqrt_price)`
   |
   = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#manual_range_contains

error: clamp-like pattern without using clamp function
   --> src/dex/math/orca.rs:168:8
    |
168 |     Ok(max(-443636, min(443636, tick)))
    |        ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ help: replace with clamp: `tick.clamp(-443636, 443636)`
    |
    = note: clamp will panic if max < min
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#manual_clamp

error: manual `!RangeInclusive::contains` implementation
   --> src/dex/math/orca.rs:197:8
    |
197 |     if sqrt_price < MIN_SQRT_PRICE || sqrt_price > MAX_SQRT_PRICE {
    |        ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ help: use: `!(MIN_SQRT_PRICE..=MAX_SQRT_PRICE).contains(&sqrt_price)`
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#manual_range_contains

error: manual `!RangeInclusive::contains` implementation
   --> src/dex/math/orca.rs:201:8
    |
201 |     if tick_current < -443636 || tick_current > 443636 {
    |        ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ help: use: `!(-443636..=443636).contains(&tick_current)`
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#manual_range_contains

error: manual implementation of an assign operation
   --> src/dex/math.rs:158:17
    |
158 |                 bin_multiplier = bin_multiplier * (Decimal::ONE + step_multiplier);
    |                 ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ help: replace it with: `bin_multiplier *= (Decimal::ONE + step_multiplier)`
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#assign_op_pattern

error: manual implementation of an assign operation
   --> src/dex/math.rs:163:17
    |
163 |                 bin_multiplier = bin_multiplier / (Decimal::ONE + step_multiplier);
    |                 ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ help: replace it with: `bin_multiplier /= (Decimal::ONE + step_multiplier)`
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#assign_op_pattern

error: you should consider adding a `Default` implementation for `Metrics`
  --> src/local_metrics/metrics.rs:22:5
   |
22 | /     pub fn new() -> Self {
23 | |         Self {
24 | |             pools_new: AtomicU64::new(0),
25 | |             pools_updated: AtomicU64::new(0),
...  |
35 | |     }
   | |_____^
   |
   = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#new_without_default
help: try adding this
   |
20 + impl Default for Metrics {
21 +     fn default() -> Self {
22 +         Self::new()
23 +     }
24 + }
   |

error: you should consider adding a `Default` implementation for `AtomicBalanceOperations`
   --> src/monitoring/balance_monitor_enhanced.rs:478:5
    |
478 | /     pub fn new() -> Self {
479 | |         Self {
480 | |             balance_locks: Arc::new(DashMap::new()),
481 | |         }
482 | |     }
    | |_____^
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#new_without_default
help: try adding this
    |
477 + impl Default for AtomicBalanceOperations {
478 +     fn default() -> Self {
479 +         Self::new()
480 +     }
481 + }
    |

error: this function has too many arguments (12/7)
   --> src/paper_trading/reporter.rs:154:5
    |
154 | /     pub fn create_trade_log_entry(
155 | |         opportunity: &MultiHopArbOpportunity,
156 | |         amount_in: u64,
157 | |         amount_out: u64,
...   |
166 | |         account_creation_fees: u64,
167 | |     ) -> TradeLogEntry {
    | |______________________^
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#too_many_arguments

error: accessing first element with `t.dex_route.get(0)`
   --> src/paper_trading/reporter.rs:311:26
    |
311 |                     dex: t.dex_route.get(0).cloned().unwrap_or_default(),
    |                          ^^^^^^^^^^^^^^^^^^ help: try: `t.dex_route.first()`
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#get_first
    = note: `-D clippy::get-first` implied by `-D warnings`
    = help: to override `-D warnings` add `#[allow(clippy::get_first)]`

error: accessing first element with `t.dex_route.get(0)`
   --> src/paper_trading/reporter.rs:326:26
    |
326 |                     dex: t.dex_route.get(0).cloned().unwrap_or_default(),
    |                          ^^^^^^^^^^^^^^^^^^ help: try: `t.dex_route.first()`
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#get_first

error: the borrowed expression implements the required traits
   --> src/paper_trading/reporter.rs:364:26
    |
364 |           wtr.write_record(&[
    |  __________________________^
365 | |             "timestamp",
366 | |             "opportunity_id",
367 | |             "token_in",
...   |
380 | |             "account_creation_fees",
381 | |         ])?;
    | |_________^
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#needless_borrows_for_generic_args
help: change this to
    |
364 ~         wtr.write_record([
365 +             "timestamp",
366 +             "opportunity_id",
367 +             "token_in",
368 +             "token_out",
369 +             "amount_in",
370 +             "amount_out",
371 +             "expected_profit",
372 +             "actual_profit",
373 +             "slippage_applied",
374 +             "fees_paid",
375 +             "execution_success",
376 +             "failure_reason",
377 +             "dex_route",
378 +             "gas_cost",
379 +             "rent_paid",
380 +             "account_creation_fees",
381 ~         ])?;
    |

error: accessing first element with `last_trade.dex_route.get(0)`
   --> src/paper_trading/reporter.rs:515:13
    |
515 |             last_trade.dex_route.get(0).cloned().unwrap_or_default(),
    |             ^^^^^^^^^^^^^^^^^^^^^^^^^^^ help: try: `last_trade.dex_route.first()`
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#get_first

error: `assert!(true)` will be optimized out by the compiler
   --> src/dex/clients/phoenix.rs:542:31
    |
542 |             OrderSide::Bid => assert!(true),
    |                               ^^^^^^^^^^^^^
    |
    = help: remove it
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#assertions_on_constants
    = note: `-D clippy::assertions-on-constants` implied by `-D warnings`
    = help: to override `-D warnings` add `#[allow(clippy::assertions_on_constants)]`

error: `assert!(false)` should probably be replaced
   --> src/dex/clients/phoenix.rs:543:31
    |
543 |             OrderSide::Ask => assert!(false),
    |                               ^^^^^^^^^^^^^^
    |
    = help: use `panic!()` or `unreachable!()`
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#assertions_on_constants

error: `assert!(false)` should probably be replaced
   --> src/dex/clients/phoenix.rs:547:31
    |
547 |             OrderSide::Bid => assert!(false),
    |                               ^^^^^^^^^^^^^^
    |
    = help: use `panic!()` or `unreachable!()`
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#assertions_on_constants

error: `assert!(true)` will be optimized out by the compiler
   --> src/dex/clients/phoenix.rs:548:31
    |
548 |             OrderSide::Ask => assert!(true),
    |                               ^^^^^^^^^^^^^
    |
    = help: remove it
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#assertions_on_constants

error: `assert!(true)` will be optimized out by the compiler
   --> src/dex/clients/phoenix.rs:559:34
    |
559 |             OrderType::Market => assert!(true),
    |                                  ^^^^^^^^^^^^^
    |
    = help: remove it
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#assertions_on_constants

error: `assert!(false)` should probably be replaced
   --> src/dex/clients/phoenix.rs:560:18
    |
560 |             _ => assert!(false),
    |                  ^^^^^^^^^^^^^^
    |
    = help: use `panic!()` or `unreachable!()`
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#assertions_on_constants

error: `assert!(true)` will be optimized out by the compiler
   --> src/dex/clients/phoenix.rs:564:33
    |
564 |             OrderType::Limit => assert!(true),
    |                                 ^^^^^^^^^^^^^
    |
    = help: remove it
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#assertions_on_constants

error: `assert!(false)` should probably be replaced
   --> src/dex/clients/phoenix.rs:565:18
    |
565 |             _ => assert!(false),
    |                  ^^^^^^^^^^^^^^
    |
    = help: use `panic!()` or `unreachable!()`
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#assertions_on_constants

error: clamp-like pattern without using clamp function
   --> src/performance/cache.rs:514:5
    |
514 |     (base_freshness - volatility_penalty).max(0.0).min(1.0)
    |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ help: replace with clamp: `(base_freshness - volatility_penalty).clamp(0.0, 1.0)`
    |
    = note: clamp will panic if max < min, min.is_nan(), or max.is_nan()
    = note: clamp returns NaN if the input is NaN
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#manual_clamp

error: you should consider adding a `Default` implementation for `MetricsCollector`
   --> src/performance/metrics.rs:214:5
    |
214 | /     pub fn new() -> Self {
215 | |         let mut system_info = System::new_all();
216 | |         system_info.refresh_all();
...   |
227 | |     }
    | |_____^
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#new_without_default
help: try adding this
    |
212 + impl Default for MetricsCollector {
213 +     fn default() -> Self {
214 +         Self::new()
215 +     }
216 + }
    |

error: clamp-like pattern without using clamp function
   --> src/performance/metrics.rs:332:9
    |
332 |         score.max(0.0).min(1.0)
    |         ^^^^^^^^^^^^^^^^^^^^^^^ help: replace with clamp: `score.clamp(0.0, 1.0)`
    |
    = note: clamp will panic if max < min, min.is_nan(), or max.is_nan()
    = note: clamp returns NaN if the input is NaN
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#manual_clamp

error: you should consider adding a `Default` implementation for `TokenMetadataCache`
  --> src/solana/accounts.rs:39:5
   |
39 | /     pub fn new() -> Self {
40 | |         Self {
41 | |             cache: Arc::new(RwLock::new(HashMap::new())),
42 | |         }
43 | |     }
   | |_____^
   |
   = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#new_without_default
help: try adding this
   |
37 + impl Default for TokenMetadataCache {
38 +     fn default() -> Self {
39 +         Self::new()
40 +     }
41 + }
   |

error: casting the result of `i64::abs()` to u64
   --> src/solana/balance_monitor.rs:560:35
    |
560 |                 let discrepancy = (opt_balance as i64 - conf_balance as i64).abs() as u64;
    |                                   ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ help: replace with: `(opt_balance as i64 - conf_balance as i64).unsigned_abs()`
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#cast_abs_to_unsigned
    = note: `-D clippy::cast-abs-to-unsigned` implied by `-D warnings`
    = help: to override `-D warnings` add `#[allow(clippy::cast_abs_to_unsigned)]`

error: large size difference between variants
   --> src/solana/event_driven_balance.rs:90:1
    |
90  | /   pub enum BalanceEventTrigger {
91  | | /     WebhookEvent {
92  | | |         notification: HeliusWebhookNotification,
93  | | |         pool_event: PoolUpdateEvent,
94  | | |     },
    | | |_____- the largest variant contains at least 464 bytes
95  | | /     DirectBalanceChange {
96  | | |         account: Pubkey,
97  | | |         new_balance: u64,
98  | | |         change_amount: i64,
99  | | |         trigger_source: String,
100 | | |     },
    | | |_____- the second-largest variant contains at least 72 bytes
...   |
103 | |       },
104 | |   }
    | |___^ the entire enum is at least 464 bytes
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#large_enum_variant
    = note: `-D clippy::large-enum-variant` implied by `-D warnings`
    = help: to override `-D warnings` add `#[allow(clippy::large_enum_variant)]`
help: consider boxing the large fields to reduce the total size of the enum
    |
92  -         notification: HeliusWebhookNotification,
92  +         notification: Box<HeliusWebhookNotification>,
    |

error: casting the result of `i64::abs()` to u64
   --> src/solana/event_driven_balance.rs:428:20
    |
428 |                 if change_amount.abs() as u64 >= config.balance_change_threshold {
    |                    ^^^^^^^^^^^^^^^^^^^^^^^^^^ help: replace with: `change_amount.unsigned_abs()`
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#cast_abs_to_unsigned

error: called `.as_ref().map(|v| v.as_slice())` on an `Option` value
   --> src/solana/rpc.rs:214:28
    |
214 |         let accounts_ref = accounts.as_ref().map(|v| v.as_slice()).unwrap_or(&[]);
    |                            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ help: consider using as_deref: `accounts.as_deref()`
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#option_as_ref_deref
    = note: `-D clippy::option-as-ref-deref` implied by `-D warnings`
    = help: to override `-D warnings` add `#[allow(clippy::option_as_ref_deref)]`

error: accessing first element with `arr.get(0)`
   --> src/streams/solana_stream_filter.rs:292:65
    |
292 | ...                   .and_then(|arr| arr.get(0))
    |                                       ^^^^^^^^^^ help: try: `arr.first()`
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#get_first

error: accessing first element with `arr.get(0)`
   --> src/streams/solana_stream_filter.rs:377:85
    |
377 | ...                   .and_then(|arr| arr.get(0))
    |                                       ^^^^^^^^^^ help: try: `arr.first()`
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#get_first

error: you should consider adding a `Default` implementation for `TestSuiteRunner`
  --> src/testing/mod.rs:46:5
   |
46 | /     pub fn new() -> Self {
47 | |         Self {
48 | |             results: Vec::new(),
49 | |             start_time: Instant::now(),
50 | |         }
51 | |     }
   | |_____^
   |
   = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#new_without_default
help: try adding this
   |
45 + impl Default for TestSuiteRunner {
46 +     fn default() -> Self {
47 +         Self::new()
48 +     }
49 + }
   |

error: you should consider adding a `Default` implementation for `EphemeralWallet`
  --> src/wallet/wallet_pool.rs:53:5
   |
53 | /     pub fn new() -> Self {
54 | |         let now = Instant::now();
55 | |         Self {
56 | |             keypair: Keypair::new(),
...  |
61 | |     }
   | |_____^
   |
   = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#new_without_default
help: try adding this
   |
52 + impl Default for EphemeralWallet {
53 +     fn default() -> Self {
54 +         Self::new()
55 +     }
56 + }
   |

error: this `impl` can be derived
   --> src/wallet/wallet_pool.rs:292:1
    |
292 | / impl Default for WalletPoolStats {
293 | |     fn default() -> Self {
294 | |         Self {
295 | |             total_wallets: 0,
...   |
304 | | }
    | |_^
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#derivable_impls
help: replace the manual implementation with a derive attribute
    |
282 + #[derive(Default)]
283 ~ pub struct WalletPoolStats {
    |

error: use of `default` to create a unit struct
  --> src/webhooks/helius.rs:26:13
   |
26 |         Self::default()
   |             ^^^^^^^^^^^ help: remove this call to `default`
   |
   = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#default_constructed_unit_structs
   = note: `-D clippy::default-constructed-unit-structs` implied by `-D warnings`
   = help: to override `-D warnings` add `#[allow(clippy::default_constructed_unit_structs)]`

error: very complex type used. Consider factoring parts into `type` definitions
  --> src/webhooks/processor.rs:15:16
   |
15 |     callbacks: Arc<Mutex<Vec<Box<dyn Fn(PoolUpdateEvent) + Send + Sync>>>>,
   |                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
   |
   = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#type_complexity

error: you should consider adding a `Default` implementation for `PoolUpdateProcessor`
  --> src/webhooks/processor.rs:26:5
   |
26 | /     pub fn new() -> Self {
27 | |         Self {
28 | |             callbacks: Arc::new(Mutex::new(Vec::new())),
29 | |         }
30 | |     }
   | |_____^
   |
   = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#new_without_default
help: try adding this
   |
24 + impl Default for PoolUpdateProcessor {
25 +     fn default() -> Self {
26 +         Self::new()
27 +     }
28 + }
   |

error: use of `unwrap_or` to construct default value
   --> src/webhooks/processor.rs:108:91
    |
108 |         let input_token_mint = tx.dex_swaps.first().and_then(|s| s.token_in.parse().ok()).unwrap_or(Pubkey::default());
    |                                                                                           ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ help: try: `unwrap_or_default()`
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#unwrap_or_default

error: use of `unwrap_or` to construct default value
   --> src/webhooks/processor.rs:109:92
    |
109 |         let output_token_mint = tx.dex_swaps.last().and_then(|s| s.token_out.parse().ok()).unwrap_or(Pubkey::default());
    |                                                                                            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ help: try: `unwrap_or_default()`
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#unwrap_or_default

error: field assignment outside of initializer for an instance created with Default::default()
   --> src/performance/cache.rs:556:9
    |
556 |         config.pool_ttl = Duration::from_millis(50); // Very short TTL for testing
    |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    |
note: consider initializing the variable with `performance::cache::CacheConfig { pool_ttl: Duration::from_millis(50), ..Default::default() }` and removing relevant reassignments
   --> src/performance/cache.rs:555:9
    |
555 |         let mut config = CacheConfig::default();
    |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#field_reassign_with_default

error: field assignment outside of initializer for an instance created with Default::default()
   --> src/solana/balance_monitor.rs:769:9
    |
769 |         config.safety_mode_threshold_pct = 10.0; // 10% threshold
    |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    |
note: consider initializing the variable with `solana::balance_monitor::BalanceMonitorConfig { safety_mode_threshold_pct: 10.0, ..Default::default() }` and removing relevant reassignments
   --> src/solana/balance_monitor.rs:768:9
    |
768 |         let mut config = BalanceMonitorConfig::default();
    |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#field_reassign_with_default

error: use of `or_insert_with` to construct default value
   --> src/websocket/price_feeds.rs:285:18
    |
285 |                 .or_insert_with(Vec::new)
    |                  ^^^^^^^^^^^^^^^^^^^^^^^^ help: try: `or_default()`
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#unwrap_or_default

error: module has the same name as its containing module
  --> src/wallet/tests.rs:6:1
   |
6  | / mod tests {
7  | |     use crate::{
8  | |         arbitrage::JitoClientConfig,
9  | |         wallet::{WalletJitoConfig, WalletJitoIntegration, WalletPoolConfig},
...  |
94 | | }
   | |_^
   |
   = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#module_inception

error: useless use of `vec!`
   --> src/testing/mock_dex.rs:317:34
    |
317 |               let error_messages = vec![
    |  __________________________________^
318 | |                 "Insufficient liquidity",
319 | |                 "Slippage tolerance exceeded",
320 | |                 "Network congestion",
321 | |                 "Pool state changed",
322 | |                 "Transaction timeout",
323 | |             ];
    | |_____________^
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#useless_vec
    = note: `-D clippy::useless-vec` implied by `-D warnings`
    = help: to override `-D warnings` add `#[allow(clippy::useless_vec)]`
help: you can use an array directly
    |
317 ~             let error_messages = ["Insufficient liquidity",
318 +                 "Slippage tolerance exceeded",
319 +                 "Network congestion",
320 +                 "Pool state changed",
321 ~                 "Transaction timeout"];
    |

error: could not compile `solana-arb-bot` (lib) due to 122 previous errors
warning: build failed, waiting for other jobs to finish...
error: useless use of `vec!`
   --> src/arbitrage/routing/mev_protection.rs:748:26
    |
748 |           let strategies = vec![
    |  __________________________^
749 | |             MevProtectionStrategy::StandardProtection,
750 | |             MevProtectionStrategy::JitoProtection,
751 | |         ];
    | |_________^
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#useless_vec
    = note: `-D clippy::useless-vec` implied by `-D warnings`
    = help: to override `-D warnings` add `#[allow(clippy::useless_vec)]`
help: you can use an array directly
    |
748 ~         let strategies = [MevProtectionStrategy::StandardProtection,
749 ~             MevProtectionStrategy::JitoProtection];
    |

error: useless use of `vec!`
   --> src/testing/mock_dex.rs:317:34
    |
317 |               let error_messages = vec![
    |  __________________________________^
318 | |                 "Insufficient liquidity",
319 | |                 "Slippage tolerance exceeded",
320 | |                 "Network congestion",
321 | |                 "Pool state changed",
322 | |                 "Transaction timeout",
323 | |             ];
    | |_____________^
    |
    = help: for further information visit https://rust-lang.github.io/rust-clippy/master/index.html#useless_vec
help: you can use an array directly
    |
317 ~             let error_messages = ["Insufficient liquidity",
318 +                 "Slippage tolerance exceeded",
319 +                 "Network congestion",
320 +                 "Pool state changed",
321 ~                 "Transaction timeout"];
    |

error: could not compile `solana-arb-bot` (lib test) due to 142 previous errors
@kvelkov ➜ /workspaces/chilliez (main) $ 