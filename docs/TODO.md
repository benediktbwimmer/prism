# TODO

- Create the follow-up coordination trust-hardening plan. Start by auditing protected `.prism` state for correctness and tamper resistance before designing the shared-coordination hardening work.
- Register and ship an official PRISM GitHub OAuth app client id for the `github-device-flow` human bootstrap path so end users do not need to provision `PRISM_GITHUB_OAUTH_CLIENT_ID` themselves.
- Delete the remaining hidden `#[cfg(any())]` legacy plan-node and plan-edge tests in `crates/prism-mcp/src/tests.rs` so the coordination graph rewrite cleanup leaves no dormant legacy mutation coverage behind.
