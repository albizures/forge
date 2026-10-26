import { existsSync, statSync } from "node:fs";
import { readFile, stat } from "node:fs/promises";
import { isAbsolute } from "node:path";
import { Type } from "typebox";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";

const ASSIGNMENT_ID = "FORGE_ASSIGNMENT_ID";
const DAEMON_URL = "FORGE_ASSIGNMENT_DAEMON_URL";
const SECRET_FILE = "FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE";

type Configuration = {
	assignmentId: string;
	daemonUrl: string;
	secretFile: string;
};

type RpcSuccess = { jsonrpc: "2.0"; id: string; result: unknown };
type RpcFailure = { jsonrpc: "2.0"; id: string; error: { code: number; message: string; data?: unknown } };

function configurationFromEnvironment(environment = process.env): Configuration | undefined {
	const assignmentId = environment[ASSIGNMENT_ID]?.trim();
	const daemonUrl = environment[DAEMON_URL]?.trim();
	const secretFile = environment[SECRET_FILE]?.trim();
	if (!assignmentId || !daemonUrl || !secretFile || !isAbsolute(secretFile)) return undefined;

	try {
		const url = new URL(daemonUrl);
		if ((url.protocol !== "http:" && url.protocol !== "https:") || !existsSync(secretFile) || !statSync(secretFile).isFile()) return undefined;
		return { assignmentId, daemonUrl: url.toString(), secretFile };
	} catch {
		return undefined;
	}
}

async function configurationDiagnostic(configuration: Configuration | undefined): Promise<string> {
	if (!configuration) {
		return "Forge subscription tools are unavailable: this is not a valid Assignment Capability Delivery session.";
	}
	try {
		if (!(await stat(configuration.secretFile)).isFile()) throw new Error("not a file");
		return `Forge subscription tools are available for Assignment ${configuration.assignmentId}.`;
	} catch {
		return `Forge subscription tools are unavailable: the Assignment Capability secret file cannot be used for Assignment ${configuration.assignmentId}.`;
	}
}

function expectedOutcome(error: RpcFailure["error"]): Record<string, unknown> | undefined {
	const code = typeof error.data === "string" ? error.data : undefined;
	if (code === "assignment-capability-denied") return { outcome: "authorization_denied" };
	if (code === "subscription-expired") return { outcome: "subscription_expired" };
	if (code === "subscription-not-found") return { outcome: "subscription_deleted" };
	return undefined;
}

function result(content: Record<string, unknown>) {
	const text = JSON.stringify(content);
	return {
		content: [{ type: "text" as const, text }],
		details: content,
		structuredContent: content,
	};
}

function isRpcResponse(value: unknown): value is RpcSuccess | RpcFailure {
	return typeof value === "object" && value !== null && "jsonrpc" in value && (value as { jsonrpc: unknown }).jsonrpc === "2.0";
}

async function call(configuration: Configuration, method: string, params: Record<string, unknown>, signal?: AbortSignal) {
	// Deliberately read at call time: the bearer never enters extension state, diagnostics, or results.
	const capabilityId = (await readFile(configuration.secretFile, "utf8")).trim();
	if (!capabilityId) return { outcome: "not_configured" };

	const id = crypto.randomUUID();
	let response: Response;
	try {
		response = await fetch(configuration.daemonUrl, {
			method: "POST",
			headers: { "content-type": "application/json" },
			body: JSON.stringify({
				jsonrpc: "2.0",
				id,
				method,
				params: { assignment_id: configuration.assignmentId, capability_id: capabilityId, correlation_id: crypto.randomUUID(), ...params },
			}),
			signal,
		});
	} catch (error) {
		throw new Error(`Forge subscription request failed: ${error instanceof Error ? error.message : "transport failure"}`);
	}
	if (!response.ok) throw new Error(`Forge subscription request failed with HTTP ${response.status}`);

	let body: unknown;
	try {
		body = await response.json();
	} catch {
		throw new Error("Forge subscription request returned malformed JSON");
	}
	if (!isRpcResponse(body) || body.id !== id || (!("result" in body) && !("error" in body))) {
		throw new Error("Forge subscription request returned an invalid JSON-RPC response");
	}
	if ("error" in body) {
		const outcome = expectedOutcome(body.error);
		if (outcome) return outcome;
		throw new Error(`Forge subscription request failed: ${body.error.message}`);
	}
	return body.result;
}

const subscriptionId = Type.Integer({ minimum: 1, description: "Forge Record Update Subscription ID" });

export default function forgeSubscriptionsExtension(pi: ExtensionAPI) {
	const configuration = configurationFromEnvironment();

	pi.registerCommand("forge-subscriptions", {
		description: "Show safe Forge subscription capability diagnostics",
		handler: async (_args, ctx) => ctx.ui.notify(await configurationDiagnostic(configuration), configuration ? "info" : "warning"),
	});

	// Do not even register model-callable tools outside a valid launch configuration.
	if (!configuration) return;

	pi.registerTool({
		name: "forge_subscribe",
		label: "Forge Subscribe",
		description: "Create or return an idempotent Forge Record Update Subscription for explicit Record IDs. Reconcile those Records before relying on future notifications.",
		parameters: Type.Object({
			target_record_ids: Type.Array(Type.Integer({ minimum: 1 }), { minItems: 1, description: "Explicit Forge Record IDs" }),
			kind_filters: Type.Optional(Type.Array(Type.String(), { description: "Optional Record Update kind filters" })),
		}),
		async execute(_toolCallId, params, signal) {
			const response = await call(configuration, "agentSubscription.create", {
				target_record_ids: params.target_record_ids,
				kind_filters: params.kind_filters ?? [],
			}, signal);
			if (typeof response === "object" && response !== null) {
				const subscription = response as Record<string, unknown>;
				return result("outcome" in subscription ? subscription : { ...subscription, reconcile_required: true });
			}
			throw new Error("Forge subscription request returned an invalid subscription");
		},
	});

	for (const [name, method, description] of [
		["forge_renew_subscription", "agentSubscription.renew", "Extend a Forge Record Update Subscription lease."],
		["forge_delete_subscription", "agentSubscription.delete", "Delete a Forge Record Update Subscription."],
	] as const) {
		pi.registerTool({
			name,
			label: name === "forge_renew_subscription" ? "Forge Renew Subscription" : "Forge Delete Subscription",
			description,
			parameters: Type.Object({ subscription_id: subscriptionId }),
			async execute(_toolCallId, params, signal) {
				const response = await call(configuration, method, { subscription_id: params.subscription_id }, signal);
				if (name === "forge_delete_subscription") {
					if (typeof response === "object" && response !== null && "outcome" in response) return result(response as Record<string, unknown>);
					return result(response === undefined ? { outcome: "deleted" } : { outcome: "deleted", response });
				}
				if (typeof response === "object" && response !== null) return result(response as Record<string, unknown>);
				throw new Error("Forge subscription request returned an invalid subscription");
			},
		});
	}
}
