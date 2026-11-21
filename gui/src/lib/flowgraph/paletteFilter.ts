import type { FlowgraphNodeSpec } from "../types";

/** Search all nodes without modifying the user's browsing visibility preferences. */
export function filterPalette(
	groups: { category: string; specs: FlowgraphNodeSpec[] }[],
	query: string,
	hiddenCategories: ReadonlySet<string>,
) {
	const search = query.trim().toLowerCase();
	return groups
		.map((group) => ({
			category: group.category,
			specs: search
				? group.specs.filter((spec) =>
						[spec.feature, spec.title, spec.category, spec.description ?? ""].some((value) => value.toLowerCase().includes(search)),
					)
				: group.specs,
			hidden: !search && hiddenCategories.has(group.category),
		}))
		.filter((group) => group.specs.length > 0);
}
