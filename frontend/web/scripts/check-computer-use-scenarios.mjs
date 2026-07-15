import { readFileSync, readdirSync } from "node:fs";
import { basename, dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const REQUIRED_METADATA = [
  "id",
  "priority",
  "mode",
  "personas",
  "routes",
  "fixtures",
  "environments",
];
const REQUIRED_SECTIONS = [
  "Intent",
  "Preconditions",
  "Steps",
  "Observable assertions",
  "Evidence",
  "Cleanup",
  "Stop conditions",
];
const PRIORITIES = new Set(["p0", "p1", "p2"]);
const MODES = new Set([
  "read-only",
  "disposable-account",
  "controlled-fault",
  "operator",
]);
const IMPLEMENTATION_COUPLING = [
  /data-testid/i,
  /querySelector/i,
  /\blocator\s*\(/i,
  /\bgetByRole\s*\(/i,
  /\bpage\.(?:click|fill|goto|locator)\b/i,
  /xpath/i,
  /css selector/i,
];

export class ScenarioError extends Error {
  constructor(file, message) {
    super(`${file}: ${message}`);
    this.name = "ScenarioError";
  }
}

function commaList(value) {
  return value
    .split(",")
    .map((part) => part.trim())
    .filter(Boolean);
}

function sectionBody(source, heading) {
  const marker = `## ${heading}`;
  const start = source.indexOf(marker) + marker.length;
  const next = source.indexOf("\n## ", start);
  return source.slice(start, next === -1 ? source.length : next).trim();
}

function requireBulletList(source, heading, file) {
  if (!/^[-*] \S/m.test(sectionBody(source, heading))) {
    throw new ScenarioError(
      file,
      `section "${heading}" must contain a bullet list`,
    );
  }
}

export function parseScenario(source, file = "scenario.md") {
  const lines = source.replaceAll("\r\n", "\n").split("\n");
  if (lines[0] !== "---") {
    throw new ScenarioError(file, "must begin with frontmatter delimiter ---");
  }
  const closing = lines.indexOf("---", 1);
  if (closing === -1) {
    throw new ScenarioError(file, "frontmatter is missing its closing ---");
  }

  const metadata = {};
  for (const line of lines.slice(1, closing)) {
    if (line.trim() === "") continue;
    const match = /^([a-z][a-z-]*):\s*(\S.*)$/.exec(line);
    if (!match) {
      throw new ScenarioError(file, `invalid frontmatter line: ${line}`);
    }
    const [, key, value] = match;
    if (!REQUIRED_METADATA.includes(key)) {
      throw new ScenarioError(file, `unknown metadata key "${key}"`);
    }
    if (Object.hasOwn(metadata, key)) {
      throw new ScenarioError(file, `duplicate metadata key "${key}"`);
    }
    metadata[key] = value.trim();
  }

  for (const key of REQUIRED_METADATA) {
    if (!metadata[key]) {
      throw new ScenarioError(file, `missing metadata key "${key}"`);
    }
  }
  if (!/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(metadata.id)) {
    throw new ScenarioError(file, "id must be lowercase kebab-case");
  }
  if (basename(file, ".md") !== metadata.id) {
    throw new ScenarioError(file, `filename must match id "${metadata.id}"`);
  }
  if (!PRIORITIES.has(metadata.priority)) {
    throw new ScenarioError(
      file,
      `priority must be one of ${[...PRIORITIES].join(", ")}`,
    );
  }
  if (!MODES.has(metadata.mode)) {
    throw new ScenarioError(
      file,
      `mode must be one of ${[...MODES].join(", ")}`,
    );
  }

  const lists = {};
  for (const key of ["personas", "routes", "fixtures", "environments"]) {
    lists[key] = commaList(metadata[key]);
    if (lists[key].length === 0) {
      throw new ScenarioError(file, `${key} must contain at least one value`);
    }
  }
  for (const route of lists.routes) {
    const segment = "(?:[A-Za-z0-9_-]+|:[a-z][a-z0-9_]*)";
    if (
      route !== "/" &&
      !new RegExp(`^/${segment}(?:/${segment})*$`).test(route)
    ) {
      throw new ScenarioError(file, `route must be a local app path: ${route}`);
    }
  }
  if (
    metadata.mode === "controlled-fault" &&
    !lists.environments.includes("instrumented-browser")
  ) {
    throw new ScenarioError(
      file,
      "controlled-fault scenarios require the instrumented-browser environment",
    );
  }

  const body = lines.slice(closing + 1).join("\n");
  const h1 = body.match(/^# (.+)$/gm) ?? [];
  if (h1.length !== 1) {
    throw new ScenarioError(file, "must contain exactly one level-one title");
  }
  const unknownSections = [...body.matchAll(/^## (.+)$/gm)]
    .map((match) => match[1])
    .filter((heading) => !REQUIRED_SECTIONS.includes(heading));
  if (unknownSections.length > 0) {
    throw new ScenarioError(
      file,
      `unknown level-two section "${unknownSections[0]}"`,
    );
  }
  let previous = -1;
  for (const heading of REQUIRED_SECTIONS) {
    const matches = [...body.matchAll(new RegExp(`^## ${heading}$`, "gm"))];
    if (matches.length !== 1) {
      throw new ScenarioError(
        file,
        `must contain exactly one "## ${heading}" section`,
      );
    }
    const index = matches[0].index;
    if (index <= previous) {
      throw new ScenarioError(file, `section "${heading}" is out of order`);
    }
    previous = index;
    if (!sectionBody(body, heading)) {
      throw new ScenarioError(file, `section "${heading}" must not be empty`);
    }
  }
  if (!/^\d+\. \S/m.test(sectionBody(body, "Steps"))) {
    throw new ScenarioError(
      file,
      'section "Steps" must contain a numbered list',
    );
  }
  for (const heading of [
    "Preconditions",
    "Observable assertions",
    "Evidence",
    "Cleanup",
    "Stop conditions",
  ]) {
    requireBulletList(body, heading, file);
  }
  for (const pattern of IMPLEMENTATION_COUPLING) {
    if (pattern.test(body)) {
      throw new ScenarioError(
        file,
        `contains implementation-coupled browser language matching ${pattern}`,
      );
    }
  }

  return {
    file,
    title: h1[0].slice(2),
    ...metadata,
    ...lists,
  };
}

export function validateCorpus(scenarios) {
  if (scenarios.length === 0) {
    throw new ScenarioError("scenario corpus", "must not be empty");
  }
  const ids = new Set();
  const titles = new Set();
  for (const scenario of scenarios) {
    if (ids.has(scenario.id)) {
      throw new ScenarioError(
        scenario.file,
        `duplicate scenario id "${scenario.id}"`,
      );
    }
    if (titles.has(scenario.title)) {
      throw new ScenarioError(
        scenario.file,
        `duplicate scenario title "${scenario.title}"`,
      );
    }
    ids.add(scenario.id);
    titles.add(scenario.title);
  }
  if (
    !scenarios.some(
      ({ priority, mode }) => priority === "p0" && mode === "read-only",
    )
  ) {
    throw new ScenarioError(
      "scenario corpus",
      "requires a P0 read-only journey",
    );
  }
  if (
    !scenarios.some(
      ({ priority, mode }) =>
        priority === "p0" && mode === "disposable-account",
    )
  ) {
    throw new ScenarioError(
      "scenario corpus",
      "requires a P0 disposable-account journey",
    );
  }
  return scenarios;
}

export function loadCorpus(directory) {
  const files = readdirSync(directory)
    .filter((file) => file.endsWith(".md"))
    .sort();
  return validateCorpus(
    files.map((file) =>
      parseScenario(readFileSync(join(directory, file), "utf8"), file),
    ),
  );
}

function summary(scenarios) {
  const priorities = Object.fromEntries(
    [...PRIORITIES].map((priority) => [
      priority,
      scenarios.filter((scenario) => scenario.priority === priority).length,
    ]),
  );
  const modes = Object.fromEntries(
    [...MODES]
      .map((mode) => [
        mode,
        scenarios.filter((scenario) => scenario.mode === mode).length,
      ])
      .filter(([, count]) => count > 0),
  );
  return { count: scenarios.length, priorities, modes };
}

function runCli() {
  const args = process.argv.slice(2);
  if (
    args.length > 1 ||
    args.some((arg) => !["--json", "--list"].includes(arg))
  ) {
    throw new Error("usage: check-computer-use-scenarios.mjs [--list|--json]");
  }
  const scriptDir = dirname(fileURLToPath(import.meta.url));
  const scenarios = loadCorpus(join(scriptDir, "../computer-use/scenarios"));
  if (args.includes("--json")) {
    console.log(
      JSON.stringify({ summary: summary(scenarios), scenarios }, null, 2),
    );
    return;
  }
  if (args.includes("--list")) {
    for (const scenario of scenarios) {
      console.log(
        `${scenario.priority.toUpperCase()}  ${scenario.mode.padEnd(18)}  ${scenario.id} — ${scenario.title}`,
      );
    }
    return;
  }
  const result = summary(scenarios);
  console.log(
    `computer-use scenarios: ${result.count} valid; priorities=${JSON.stringify(result.priorities)} modes=${JSON.stringify(result.modes)}`,
  );
}

if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(process.argv[1]).href
) {
  try {
    runCli();
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error));
    process.exitCode = 1;
  }
}
