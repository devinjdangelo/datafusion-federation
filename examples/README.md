## DataFusion Federation Examples

To run the examples, use the `cargo run` command:

```bash
# Run the `sqlite.rs` example:
cargo run --example sqlite
```

- [sqlite](./examples/sqlite.rs): federate an entire query to a SQLite database.
- [sqlite-partial](./examples/sqlite-partial.rs): federate parts of a query to two separate SQLite database instances.
