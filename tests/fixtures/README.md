# Xact detection regressions

These source snapshots are text fixtures, not compiled or executed. The `.rs.txt`
extension keeps a normal workspace audit from treating them as live requests.

Retrieved from `elci-group/xact` on 2026-10-23:

- `crates/xact-tell/src/anthropic.rs`, blob `2b3d30b68fbcad45ca1294aaaa6b78af2c6cdd4a`
- `crates/xact-tell/src/openai_compatible.rs`, blob `c74279e8ee37ea0461f2a4a2d97b627d707fadac`

Discovery tests scan each complete file: exactly one implicit API action is
expected, while metadata GET requests, documentation and test regions are ignored.
A second test inserts a numeric temperature and expects one explicit action with
preserved decimal precision, without an extra implicit action.
