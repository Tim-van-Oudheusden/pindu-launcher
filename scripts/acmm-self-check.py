#!/usr/bin/env python3
"""Score a checkout against Hive's ACMM codebase criteria and fail below a minimum level.

Ported from Tim-van-Oudheusden/Dailies (scripts/acmm-self-check.py), whose docs/research/hive-acmm.md (section 4.1
"Scoring rules", section 9 "Self-check in CI" and Appendix C) documents Hive's evaluator. The nightly compliance run
(scripts/nightly-compliance.sh) calls it, so that a renamed or deleted file that drops a level shows up as a red nightly
row instead of a surprise "[ACMM Ln] Add ..." gap issue from Hive.

Hive's ACMM Eval only checks that files exist:

- A criterion passes when any one of its paths exists; a path ending in "/" must be a directory.
- A level passes when at least 70 % of its criteria pass. There are no L1 criteria.
- The codebase level is the highest level reached by consecutive passes from L0 upward. Passing L0 alone gives L1;
  failing L0 gives 0 ("Prerequisites not met") whatever the higher levels score.

Waivers in .acmm.yml are not read: Hive never lets a waiver advance a level. Hive sees only what is pushed, so run this
on a clean checkout (as CI does); an untracked local file can make a criterion pass here and nowhere else.

Usage:
    python3 scripts/acmm-self-check.py [--root DIR] [--min-level N]

Prints a Markdown report and exits 0 when the codebase level is at least --min-level (default DEFAULT_MIN_LEVEL), 1
when it is lower.
"""

import argparse
import sys
from pathlib import Path
from typing import NamedTuple

# A level passes when at least this share of its criteria pass (acmmLevelThreshold in api_acmm_eval.go).
LEVEL_THRESHOLD = 0.70
# The level this repository has reached, so the self-check fails when a change drops it. It starts at 0 (L0
# prerequisites unmet, as on 2026-11-07); every Hive readiness issue (milestone "Hive readiness", #237) that completes a
# level raises it in the same pull request, up to 6 once Hive's target of L6 is met.
DEFAULT_MIN_LEVEL = 0
MAX_LEVEL = 6

# acmmLevelNames: the name of each codebase level. 0 is not a level of its own; it means the L1 prerequisites are unmet.
LEVEL_NAMES = {
    0: "Prerequisites not met",
    1: "Inception",
    2: "Advisory",
    3: "Quality-Gated",
    4: "Security-Aware",
    5: "Semi-Autonomous",
    6: "Fully Autonomous",
}


class Criterion(NamedTuple):
    """One entry of Hive's ``universalCriteria``, field for field."""

    key: str
    source: str
    level: int
    category: str
    name: str
    patterns: tuple[str, ...]


# Vendored from hivecommons/hive, src/pkg/dashboard/acmm_criteria.go (`universalCriteria`, 50 entries; the comment
# above it upstream still says 43), at commit 3fcbd6d1b5d40fc7658202ac66cb28149f9a81c7 (head of branch v5,
# 2026-10-15). The file is byte-identical at 1bf213d0cb772ff8425e1787586a33b01f7ea27e, the commit that
# Dailies' docs/research/hive-acmm.md section 4 reproduces. Keep the entries in upstream order and re-sync them whenever
# that file changes upstream (watch the `acmm` label and Hive's CHANGELOG).
CRITERIA: tuple[Criterion, ...] = (
    # L0 Prerequisites (8)
    Criterion(
        "acmm:prereq-test-suite",
        "acmm",
        0,
        "prerequisite",
        "Test suite",
        (
            "vitest.config.ts",
            "vitest.config.js",
            "jest.config.js",
            "jest.config.ts",
            "go.mod",
            "pytest.ini",
            "pyproject.toml",
            "test/",
            "tests/",
            "__tests__/",
            "spec/",
        ),
    ),
    Criterion(
        "acmm:prereq-e2e",
        "acmm",
        0,
        "prerequisite",
        "E2E tests",
        (
            "playwright.config.ts",
            "playwright.config.js",
            "cypress.config.ts",
            "e2e/",
            "web/e2e/",
            "web/playwright.config.ts",
            "web/playwright.config.js",
            "test/e2e/",
            "tests/e2e/",
        ),
    ),
    Criterion(
        "acmm:prereq-cicd",
        "acmm",
        0,
        "prerequisite",
        "CI/CD pipeline",
        (".github/workflows/", ".gitlab-ci.yml", "Jenkinsfile", ".circleci/"),
    ),
    Criterion(
        "acmm:prereq-pr-template",
        "acmm",
        0,
        "prerequisite",
        "PR template",
        (".github/pull_request_template.md", ".github/PULL_REQUEST_TEMPLATE.md"),
    ),
    Criterion(
        "acmm:prereq-issue-template",
        "acmm",
        0,
        "prerequisite",
        "Issue templates",
        (".github/ISSUE_TEMPLATE/", ".github/issue_template.md"),
    ),
    Criterion(
        "acmm:prereq-contrib-guide",
        "acmm",
        0,
        "prerequisite",
        "Contributing guide",
        ("CONTRIBUTING.md", "docs/contributing.md", ".github/CONTRIBUTING.md"),
    ),
    Criterion(
        "acmm:prereq-code-style",
        "acmm",
        0,
        "prerequisite",
        "Code style config",
        (
            ".eslintrc",
            ".eslintrc.json",
            ".eslintrc.js",
            "eslint.config.js",
            "eslint.config.mjs",
            ".prettierrc",
            ".prettierrc.json",
            "prettier.config.js",
            "ruff.toml",
            ".golangci.yml",
            ".golangci.yaml",
            ".golangci.toml",
            ".golangci.json",
            "biome.json",
            ".shellcheckrc",
            ".editorconfig",
            ".clang-format",
            "checkstyle.xml",
            "config/checkstyle/checkstyle.xml",
            ".rubocop.yml",
            ".rubocop.yaml",
            "rustfmt.toml",
            ".rustfmt.toml",
            "clippy.toml",
            ".clippy.toml",
            ".swiftlint.yml",
            ".stylelintrc",
            ".stylelintrc.json",
            ".stylelintrc.yaml",
            ".stylelintrc.yml",
        ),
    ),
    Criterion(
        "acmm:prereq-coverage-gate",
        "acmm",
        0,
        "prerequisite",
        "Coverage gate",
        (
            "codecov.yml",
            ".codecov.yml",
            ".github/workflows/coverage-gate.yml",
            "coverage.yml",
            ".coverage-thresholds.json",
        ),
    ),
    # L2 Advisory (8)
    Criterion(
        "acmm:claude-md", "acmm", 2, "feedback-loop", "CLAUDE.md instruction file", ("CLAUDE.md", ".claude/CLAUDE.md")
    ),
    Criterion(
        "acmm:copilot-instructions",
        "acmm",
        2,
        "feedback-loop",
        "Copilot instructions",
        (".github/copilot-instructions.md",),
    ),
    Criterion("acmm:agents-md", "acmm", 2, "feedback-loop", "AGENTS.md", ("AGENTS.md",)),
    Criterion("acmm:cursor-rules", "acmm", 2, "feedback-loop", "Cursor rules", (".cursorrules", ".cursor/rules")),
    Criterion(
        "acmm:prompts-catalog",
        "acmm",
        2,
        "feedback-loop",
        "Prompts catalog",
        ("prompts/", ".prompts/", "docs/prompts/", ".github/prompts/", ".github/agents/"),
    ),
    Criterion("acmm:editor-config", "acmm", 2, "feedback-loop", "EditorConfig", (".editorconfig",)),
    Criterion(
        "acmm:simple-skills",
        "acmm",
        2,
        "feedback-loop",
        "Simple skills",
        (".claude/skills/", ".claude/commands/", "skills/"),
    ),
    Criterion(
        "acmm:correction-capture",
        "acmm",
        2,
        "learning",
        "Correction capture",
        (".claude/memory/", ".memory/", "corrections.jsonl"),
    ),
    # L3 Quality-Gated (8)
    Criterion(
        "acmm:pr-acceptance-metric",
        "acmm",
        3,
        "feedback-loop",
        "PR acceptance metric",
        (
            "scripts/build-accm-history.mjs",
            ".github/workflows/accm-history-update.yml",
            "scripts/pr-metrics.mjs",
            ".github/workflows/pr-metrics.yml",
            "docs/metrics.md",
        ),
    ),
    Criterion(
        "acmm:pr-review-rubric",
        "acmm",
        3,
        "feedback-loop",
        "PR review rubric",
        (
            ".github/workflows/review.yml",
            "docs/review-rubric.md",
            ".github/review-checklist.md",
            ".github/prompts/review.md",
            "docs/qa/",
        ),
    ),
    Criterion(
        "acmm:quality-dashboard",
        "acmm",
        3,
        "observability",
        "Quality dashboard",
        (
            "public/analytics.js",
            "web/public/analytics.js",
            "web/src/components/analytics/",
            "docs/quality.md",
            ".github/workflows/quality-report.yml",
            "docs/AI-QUALITY-ASSURANCE.md",
        ),
    ),
    Criterion(
        "acmm:ci-matrix",
        "acmm",
        3,
        "feedback-loop",
        "CI test matrix",
        (".github/workflows/ci.yml", ".github/workflows/test.yml", ".github/workflows/build.yml"),
    ),
    Criterion(
        "acmm:layered-safety",
        "acmm",
        3,
        "governance",
        "Layered safety model",
        (".claude/settings.json", ".claude/settings.local.json"),
    ),
    Criterion(
        "acmm:mechanical-enforcement", "acmm", 3, "governance", "Mechanical enforcement", (".claude/settings.json",)
    ),
    Criterion(
        "acmm:session-summary",
        "acmm",
        3,
        "learning",
        "Session summary artifact",
        (".claude/session-summary.md", ".claude/checkpoint.md"),
    ),
    Criterion("acmm:structural-gates", "acmm", 3, "traceability", "Structural gates", (".claude/settings.json",)),
    # L4 Security-Aware (9)
    Criterion(
        "acmm:auto-qa-tuning",
        "acmm",
        4,
        "self-tuning",
        "Auto-QA self-tuning",
        (".github/auto-qa-tuning.json", ".github/workflows/auto-qa.yml", "scripts/auto-qa-tuner.mjs"),
    ),
    Criterion(
        "acmm:nightly-compliance",
        "acmm",
        4,
        "feedback-loop",
        "Nightly compliance",
        (
            ".github/workflows/nightly-compliance.yml",
            ".github/workflows/nightly.yml",
            ".github/workflows/nightly-test.yml",
            ".github/workflows/nightly-test-suite.yml",
        ),
    ),
    Criterion(
        "acmm:copilot-review-apply",
        "acmm",
        4,
        "feedback-loop",
        "Automated review application",
        (
            ".github/workflows/copilot-review-apply.yml",
            ".github/workflows/apply-copilot.yml",
            ".github/workflows/ai-fix.yml",
            ".github/workflows/auto-review.yml",
        ),
    ),
    Criterion(
        "acmm:auto-label",
        "acmm",
        4,
        "feedback-loop",
        "Automated issue labeling",
        (".github/labeler.yml", ".github/workflows/labeler.yml", ".github/workflows/triage.yml"),
    ),
    Criterion(
        "acmm:ai-fix-workflow",
        "acmm",
        4,
        "feedback-loop",
        "AI-fix-requested workflow",
        (".github/workflows/ai-fix.yml", ".github/workflows/ai-fix-requested.yml", ".github/workflows/claude.yml"),
    ),
    Criterion(
        "acmm:tier-classifier",
        "acmm",
        4,
        "governance",
        "Change classification",
        (
            ".github/workflows/tier-classifier.yml",
            "docs/risk-tiers.md",
            ".github/risk-tiers.yml",
            ".github/workflows/pr-size.yml",
        ),
    ),
    Criterion(
        "acmm:security-ai-md",
        "acmm",
        4,
        "governance",
        "AI security policy",
        ("docs/security/SECURITY-AI.md", "SECURITY-AI.md", "docs/SECURITY-AI.md"),
    ),
    Criterion(
        "acmm:session-continuity",
        "acmm",
        4,
        "learning",
        "Session continuity",
        (".claude/checkpoint.md", ".claude/session-summary.md"),
    ),
    Criterion(
        "acmm:cross-session-knowledge",
        "acmm",
        4,
        "learning",
        "Cross-session knowledge",
        ("knowledge.jsonl", ".knowledge/", "docs/reflections/"),
    ),
    # L5 Semi-Autonomous (5)
    Criterion(
        "acmm:github-actions-ai",
        "acmm",
        5,
        "feedback-loop",
        "GitHub Actions AI integration",
        (".github/workflows/claude.yml", ".github/workflows/claude-code-review.yml"),
    ),
    Criterion(
        "acmm:auto-qa-self-tuning",
        "acmm",
        5,
        "self-tuning",
        "Auto-QA with self-tuning",
        (".github/workflows/auto-qa.yml", ".github/auto-qa-tuning.json"),
    ),
    Criterion(
        "acmm:public-metrics",
        "acmm",
        5,
        "observability",
        "Public metrics",
        ("web/netlify/functions/analytics-accm.mts", "web/public/analytics.js", "docs/metrics/"),
    ),
    Criterion(
        "acmm:policy-as-code",
        "acmm",
        5,
        "governance",
        "Policy-as-code",
        ("policies/", ".github/policies/", "kyverno/", "conftest.yaml", "opa/"),
    ),
    Criterion(
        "acmm:reflection-log",
        "acmm",
        5,
        "feedback-loop",
        "Reflection log",
        (".claude/reflections/", "memory/", ".memory/", "docs/reflections/", "REFLECTIONS.md"),
    ),
    # L6 Fully Autonomous (6 acmm)
    Criterion(
        "acmm:auto-issue-gen",
        "acmm",
        6,
        "autonomy",
        "Auto issue generation",
        (
            ".github/workflows/auto-issues.yml",
            ".github/workflows/auto-issue.yml",
            ".github/workflows/issue-gen.yml",
            ".github/workflows/auto-generate-issues.yml",
            "scripts/generate-issues.mjs",
        ),
    ),
    Criterion(
        "acmm:multi-agent-orchestration",
        "acmm",
        6,
        "autonomy",
        "Multi-agent orchestration",
        (
            ".github/workflows/dispatcher.yml",
            ".github/workflows/orchestrate.yml",
            "scripts/orchestrate.mjs",
            "docs/multi-agent.md",
            ".claude/dispatcher/",
            "orchestrator/",
        ),
    ),
    Criterion(
        "acmm:strategic-dashboard",
        "acmm",
        6,
        "observability",
        "Strategic dashboard",
        (
            "web/src/components/acmm/",
            "docs/strategy.md",
            ".github/workflows/strategy-report.yml",
            "docs/autonomous-work-log.md",
        ),
    ),
    Criterion(
        "acmm:merge-queue",
        "acmm",
        6,
        "autonomy",
        "Merge queue",
        (".github/workflows/merge-queue.yml", ".github/merge-queue.yml", ".prow.yaml", "tide.yaml"),
    ),
    Criterion(
        "acmm:risk-assessment-config",
        "acmm",
        6,
        "governance",
        "Risk assessment config",
        ("risk-config.json", ".claude/risk-config.json", ".github/risk-assessment.yml"),
    ),
    Criterion(
        "acmm:observability-runbook",
        "acmm",
        6,
        "governance",
        "Observability runbook",
        ("docs/ai-ops-runbook.md", "docs/runbook/", "RUNBOOK.md"),
    ),
    # L6 Agentic Engineering Framework (6 aef)
    Criterion(
        "aef:task-traceability",
        "aef",
        6,
        "governance",
        "Task traceability",
        (".agent/tasks/", "docs/agent-tasks/", ".github/agent-log/", "agent-tasks.md"),
    ),
    Criterion(
        "aef:structural-gates",
        "aef",
        6,
        "governance",
        "Structural gates",
        ("CODEOWNERS", ".github/CODEOWNERS", ".agent/boundaries.yml", "docs/agent-boundaries.md"),
    ),
    Criterion(
        "aef:session-continuity",
        "aef",
        6,
        "governance",
        "Session continuity",
        ("CLAUDE.md", "AGENTS.md", ".cursorrules", ".github/copilot-instructions.md", "docs/agent-context.md"),
    ),
    Criterion(
        "aef:audit-trail",
        "aef",
        6,
        "governance",
        "Audit trail workflow",
        (".github/workflows/ai-audit.yml", ".github/workflows/agent-audit.yml", "scripts/ai-audit-report.mjs"),
    ),
    Criterion(
        "aef:cross-tool-config",
        "aef",
        6,
        "governance",
        "Cross-tool agent config",
        ("AGENTS.md", "docs/ai-contributors.md", ".github/ai-config.yml"),
    ),
    Criterion(
        "aef:change-classification",
        "aef",
        6,
        "governance",
        "Change classification",
        ("docs/change-classification.md", ".github/change-tiers.yml", "docs/risk-tiers.md"),
    ),
)


class LevelResult(NamedTuple):
    """The criteria of one level, split into those that passed and those that did not."""

    level: int
    met: tuple[Criterion, ...]
    missing: tuple[Criterion, ...]

    @property
    def total(self) -> int:
        """Number of criteria at this level."""
        return len(self.met) + len(self.missing)

    @property
    def score(self) -> float:
        """Share of this level's criteria that passed, from 0.0 to 1.0."""
        return len(self.met) / self.total if self.total else 0.0

    @property
    def passed(self) -> bool:
        """Whether the level reaches the threshold."""
        return self.score >= LEVEL_THRESHOLD


def pattern_exists(root: Path, pattern: str) -> bool:
    """Tell whether one criterion path exists under the repository root.

    Args:
        root: The repository root.
        pattern: A path relative to the root; a trailing "/" means it must be a directory.

    Returns:
        True when the path exists (and is a directory, for a directory pattern).
    """
    path = root / pattern.rstrip("/")
    return path.is_dir() if pattern.endswith("/") else path.exists()


def criterion_met(root: Path, criterion: Criterion) -> bool:
    """Tell whether any one of a criterion's paths exists.

    Args:
        root: The repository root.
        criterion: The criterion to check.

    Returns:
        True when at least one of its patterns exists.
    """
    return any(pattern_exists(root, pattern) for pattern in criterion.patterns)


def evaluate(root: Path, criteria: tuple[Criterion, ...] = CRITERIA) -> list[LevelResult]:
    """Check every criterion and group the outcomes by level.

    Args:
        root: The repository root.
        criteria: The criteria to check; Hive's list by default.

    Returns:
        One result per level that has criteria, in ascending level order.
    """
    results = []
    for level in sorted({criterion.level for criterion in criteria}):
        at_level = [criterion for criterion in criteria if criterion.level == level]
        met = tuple(criterion for criterion in at_level if criterion_met(root, criterion))
        missing = tuple(criterion for criterion in at_level if criterion not in met)
        results.append(LevelResult(level, met, missing))
    return results


def codebase_level(results: list[LevelResult]) -> int:
    """Compute the codebase level the way Hive's ``scoreResults`` does.

    Args:
        results: Per-level results in ascending level order, as returned by `evaluate`.

    Returns:
        The highest level reached by consecutive passes from the lowest level; passing L0 alone gives 1, and a failed
        L0 gives 0.
    """
    reached = 0
    for result in results:
        if not result.passed:
            break
        reached = max(result.level, 1)
    return reached


def level_label(level: int) -> str:
    """Name a codebase level, such as "L5 Semi-Autonomous".

    Args:
        level: A codebase level from 0 to 6.

    Returns:
        The level number and Hive's name for it.
    """
    return f"L{level} {LEVEL_NAMES[level]}"


def format_report(results: list[LevelResult], reached: int, min_level: int) -> str:
    """Render the evaluation as Markdown, listing the missing criteria of every level.

    Args:
        results: Per-level results, as returned by `evaluate`.
        reached: The codebase level, as returned by `codebase_level`.
        min_level: The level the repository must reach.

    Returns:
        The report text, ending in a newline.
    """
    verdict = "ok" if reached >= min_level else "BELOW REQUIRED LEVEL"
    lines = [
        f"ACMM codebase level: {level_label(reached)} (required: {level_label(min_level)}): {verdict}",
        "",
    ]
    for result in results:
        title = "Prerequisites" if result.level == 0 else LEVEL_NAMES[result.level]
        status = "pass" if result.passed else "FAIL"
        lines.append(f"- L{result.level} {title}: {len(result.met)}/{result.total} ({result.score:.0%}) {status}")
        lines.extend(
            f"  - missing `{criterion.key}` ({criterion.name}): none of {', '.join(criterion.patterns)}"
            for criterion in result.missing
        )
    return "\n".join(lines) + "\n"


def parse_args(argv: list[str] | None) -> argparse.Namespace:
    """Parse the command line.

    Args:
        argv: The arguments after the program name; None reads sys.argv.

    Returns:
        The parsed options, with ``root`` resolved to an absolute path.
    """
    parser = argparse.ArgumentParser(description="Score a checkout against Hive's ACMM codebase criteria.")
    parser.add_argument(
        "--root",
        type=Path,
        default=Path(__file__).resolve().parent.parent,
        help="repository root to check (default: the checkout this script is in)",
    )
    parser.add_argument(
        "--min-level",
        type=int,
        choices=range(MAX_LEVEL + 1),
        default=DEFAULT_MIN_LEVEL,
        metavar="N",
        help=f"fail when the codebase level is below N, 0 to {MAX_LEVEL} (default: {DEFAULT_MIN_LEVEL})",
    )
    args = parser.parse_args(argv)
    if not args.root.is_dir():
        parser.error(f"--root: not a directory: {args.root}")
    args.root = args.root.resolve()
    return args


def main(argv: list[str] | None = None) -> int:
    """Evaluate the repository and print the report.

    Args:
        argv: The arguments after the program name; None reads sys.argv.

    Returns:
        0 when the codebase level reaches the minimum, 1 otherwise.
    """
    args = parse_args(argv)
    results = evaluate(args.root)
    reached = codebase_level(results)
    sys.stdout.write(format_report(results, reached, args.min_level))
    return 0 if reached >= args.min_level else 1


if __name__ == "__main__":
    sys.exit(main())
