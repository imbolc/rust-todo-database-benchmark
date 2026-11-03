# Repository instructions

## Questions

- When a request is a question, answer it first. Do not implement an implied
  solution unless the user confirms.

## Languages

- Use only Rust and POSIX `sh` for repository code and tooling
- Temporary investigation scripts may use any language

## Documentation

- README.md content is written and maintained by humans. Agents may edit it,
  including links, only when explicitly requested or approved by the user.
  Otherwise, describe the proposed change and request explicit approval.
  Automatic formatting is allowed without approval.

### Completion checks

- If a task assume multiple commits do only `cargo check -q` and targeted tests
  before an intermediate commit and omit pre-commit checks with
  `git commit --no-verify`
- After completing the task fully run auto-formatters and commit normally,
  ensuring the pre-commit checks pass:

    ```sh
    cargo sort
    cargo +nightly fmt --all
    rumdl fmt -q .
    ```

### Style

- Use fully qualified paths for derive macros from external crates
- In derive lists, place built-in derives before external derives. Sort each
  group when possible.

## Git

- When all changes belong in the commit, run `git add .` to avoid triggering
  permission request
- After changes, suggest a commit message and ask if you should commit the
  changes
