#!/usr/bin/env node

import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname } from "node:path";

const OUTPUT_PATH = process.env.INSTALL_SNAPSHOT_PATH ?? "ops/metrics/install_snapshots.json";
const repoRef = process.env.GITHUB_REPOSITORY;
const fallbackOwner = process.env.GITHUB_OWNER;
const fallbackRepo = process.env.GITHUB_REPO;
const token = process.env.GITHUB_TOKEN;

const [owner, repo] = repoRef?.split("/") ?? [fallbackOwner, fallbackRepo];
if (!owner || !repo) {
  throw new Error(
    "Missing repo context. Set GITHUB_REPOSITORY or both GITHUB_OWNER and GITHUB_REPO.",
  );
}

const baseUrl = `https://api.github.com/repos/${owner}/${repo}`;

async function fetchReleases() {
  const response = await fetch(`${baseUrl}/releases?per_page=100`, {
    headers: {
      Accept: "application/vnd.github+json",
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
      "User-Agent": "resetping-install-snapshot",
      "X-GitHub-Api-Version": "2022-11-28",
    },
  });

  if (!response.ok) {
    throw new Error(`GitHub API request failed (${response.status} ${response.statusText})`);
  }

  return response.json();
}

async function loadExisting(path) {
  try {
    const content = await readFile(path, "utf8");
    const parsed = JSON.parse(content);
    if (Array.isArray(parsed)) return parsed;
  } catch {
    // ignore missing/invalid file and start fresh
  }
  return [];
}

function buildSnapshot(releases) {
  const normalizedReleases = releases.map((release) => ({
    id: release.id,
    tag_name: release.tag_name,
    name: release.name ?? null,
    draft: Boolean(release.draft),
    prerelease: Boolean(release.prerelease),
    published_at: release.published_at ?? null,
    assets: (release.assets ?? []).map((asset) => ({
      id: asset.id,
      name: asset.name,
      content_type: asset.content_type ?? null,
      size: asset.size ?? null,
      download_count: asset.download_count ?? 0,
      updated_at: asset.updated_at ?? null,
    })),
  }));

  const totalDownloads = normalizedReleases.reduce(
    (sum, release) =>
      sum + release.assets.reduce((assetSum, asset) => assetSum + (asset.download_count ?? 0), 0),
    0,
  );

  return {
    captured_at: new Date().toISOString(),
    repo: `${owner}/${repo}`,
    total_downloads: totalDownloads,
    releases: normalizedReleases,
  };
}

async function main() {
  const releases = await fetchReleases();
  const snapshot = buildSnapshot(releases);
  const history = await loadExisting(OUTPUT_PATH);
  history.push(snapshot);

  const trimmed = history.slice(-730);

  await mkdir(dirname(OUTPUT_PATH), { recursive: true });
  await writeFile(OUTPUT_PATH, `${JSON.stringify(trimmed, null, 2)}\n`, "utf8");
  console.log(
    `Wrote ${OUTPUT_PATH} (${trimmed.length} snapshots, total_downloads=${snapshot.total_downloads})`,
  );
}

await main();
