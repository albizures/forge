import { existsSync, readdirSync } from "node:fs";
import { join } from "node:path";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";

function discoverSkillDirectories(root: string): Array<string> {
	if (!existsSync(root)) {
		return [];
	}

	const entries = readdirSync(root, { withFileTypes: true });

	if (entries.some((entry) => entry.isFile() && entry.name === "SKILL.md")) {
		return [root];
	}

	return entries
		.filter(
			(entry) =>
				entry.isDirectory() &&
				!entry.name.startsWith(".") &&
				entry.name !== "node_modules",
		)
		.flatMap((entry) => discoverSkillDirectories(join(root, entry.name)))
		.sort();
}

// biome-ignore lint/style/noDefaultExport: Pi extension modules are loaded through default exports.
export default function (pi: ExtensionAPI) {
	pi.on("resources_discover", async (event) => {
		const skillCatalogPath = join(event.cwd, "skills");
		const skillPaths = discoverSkillDirectories(skillCatalogPath);

		if (skillPaths.length === 0) {
			return;
		}

		return { skillPaths };
	});
}
