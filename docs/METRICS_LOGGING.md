# Metrics Logging with `metrics-exporter-log`

## Overview

This project uses the [`metrics-exporter-log`](https://crates.io/crates/metrics-exporter-log) backend for metrics collection. This approach is:

- **Reliable**: No external server required
- **Fast**: Minimal overhead
- **Resource-light**: Metrics are logged to your application logs

## How It Works

- Metrics macros (`counter!`, `gauge!`, `histogram!`) are used throughout the codebase.
- Metrics are periodically printed to the application logs.
- You can view metrics by tailing the logs or piping them to a file.

## Setup

1. Add to `Cargo.toml`:

   ```toml
   [dependencies]
   metrics = "0.22"
   metrics-exporter-log = "0.7"
   ```

2. Initialize in `main.rs`:

   ```rust
   fn main() {
       metrics_exporter_log::init();
       // ...rest of your startup code...
   }
   ```

3. Use metrics macros as usual:

   ```rust
   use metrics::{counter, gauge, histogram};
   counter!("my_counter", 1);
   ```

## Viewing Metrics

- Run your application and look for lines like:
- For dashboards, consider piping logs to Loki/Grafana or similar tools.
