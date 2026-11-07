# The coverage floors' contract, shared by scripts/coverage-check.sh, scripts/quality-report.sh and
# scripts/auto-qa-tuner.sh (`jq -L scripts/lib 'include "coverage"; ...'`).
#
# The tuning config (.github/auto-qa-tuning.json) is {"margin": N, "floors": {"<module>": {"lines": N,
# "regions": N, "functions": N}}}. Module <module> is the crate crates/pindu-<module>. Branch coverage
# needs a nightly compiler, so it has no floor.

# The gated metrics, in report order.
def metrics: ["lines", "regions", "functions"];

# Whether the input is a valid tuning config: floors are percentages, the margin a non-negative
# number of percentage points.
def valid_config:
  def percent: type == "number" and . >= 0 and . <= 100;
  (.margin | type == "number" and . >= 0)
  and (.floors | type == "object" and length > 0
    and all(keys[]; test("^[a-z0-9][a-z0-9-]*$"))
    and all(.[]; type == "object" and keys == (metrics | sort) and all(.[]; percent)));

def config_error($path):
  "Malformed \($path): expected {\"margin\": N, \"floors\": {\"<module>\": {\"lines\": N, \"regions\": N,"
  + " \"functions\": N}}} with percentages between 0 and 100";

# Whether the input is a `cargo llvm-cov --json` export with a summary for every file.
def valid_report:
  def counts: type == "object" and (.covered | type == "number") and (.count | type == "number");
  .data | type == "array" and length > 0
    and all(.[]; .files | type == "array"
      and all(.[]; (.filename | type == "string") and (.summary as $s | all(metrics[]; $s[.] | counts))));

# {"<metric>": {covered, count}} of a cargo-llvm-cov export, summed over the files of crates/pindu-$module.
def module_counts($module):
  ("crates/pindu-" + $module + "/") as $dir
  | [.data[].files[] | select((.filename | startswith($dir)) or (.filename | contains("/" + $dir)))] as $files
  | reduce metrics[] as $m ({};
      .[$m] = {covered: ([$files[].summary[$m].covered] | add // 0), count: ([$files[].summary[$m].count] | add // 0)});
