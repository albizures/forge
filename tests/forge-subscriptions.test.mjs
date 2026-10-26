import assert from "node:assert/strict";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { createJiti } from "/home/a/.volta/tools/image/packages/@earendil-works/pi-coding-agent/lib/node_modules/@earendil-works/pi-coding-agent/node_modules/jiti/lib/jiti.mjs";

const loadExtension = createJiti(import.meta.url);
const extension = (await loadExtension.import(join(process.cwd(), ".pi/extensions/forge-subscriptions.ts"))).default;
const environmentNames = ["FORGE_ASSIGNMENT_ID", "FORGE_ASSIGNMENT_DAEMON_URL", "FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE"];

function install(pi) {
	const tools = new Map();
	const commands = new Map();
	extension({
		registerTool: (tool) => tools.set(tool.name, tool),
		registerCommand: (name, command) => commands.set(name, command),
	});
	return { tools, commands };
}

function withoutConfiguration(run) {
	const previous = Object.fromEntries(environmentNames.map((name) => [name, process.env[name]]));
	for (const name of environmentNames) delete process.env[name];
	try {
		return run();
	} finally {
		for (const name of environmentNames) {
			if (previous[name] === undefined) delete process.env[name];
			else process.env[name] = previous[name];
		}
	}
}

test("does not expose privileged tools without Assignment Capability Delivery", () => {
	withoutConfiguration(() => {
		const { tools, commands } = install();
		assert.equal(tools.size, 0);
		assert.ok(commands.has("forge-subscriptions"));
	});
});

test("uses JSON-RPC, rereads the capability file, normalizes results, and never returns the capability", async () => {
	const root = await mkdtemp(join(tmpdir(), "forge-subscriptions-test-"));
	const secretFile = join(root, "capability");
	const requests = [];
	const server = createServer(async (request, response) => {
		let raw = "";
		for await (const chunk of request) raw += chunk;
		const body = JSON.parse(raw);
		requests.push(body);
		const result = body.method === "agentSubscription.delete"
			? null
			: { id: 9, assignment_id: "assignment-1", target_record_ids: [41], kind_filters: [], start_cursor: 12, lease_expires_at_millis: 999 };
		response.setHeader("content-type", "application/json");
		response.end(JSON.stringify({ jsonrpc: "2.0", id: body.id, result }));
	});
	await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
	const address = server.address();

	try {
		await writeFile(secretFile, "first-secret\n");
		process.env.FORGE_ASSIGNMENT_ID = "assignment-1";
		process.env.FORGE_ASSIGNMENT_DAEMON_URL = `http://127.0.0.1:${address.port}`;
		process.env.FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE = secretFile;
		const { tools } = install();

		const created = await tools.get("forge_subscribe").execute("call", { target_record_ids: [41] });
		assert.equal(created.details.reconcile_required, true);
		assert.equal(created.content[0].text.includes("first-secret"), false);
		await writeFile(secretFile, "second-secret\n");
		const deleted = await tools.get("forge_delete_subscription").execute("call", { subscription_id: 9 });
		assert.deepEqual(deleted.details, { outcome: "deleted", response: null });
		assert.deepEqual(requests.map((request) => request.method), ["agentSubscription.create", "agentSubscription.delete"]);
		assert.equal(requests[0].params.capability_id, "first-secret");
		assert.equal(requests[1].params.capability_id, "second-secret");
		assert.deepEqual(requests[0].params.target_record_ids, [41]);
		await writeFile(secretFile, "\n");
		assert.deepEqual((await tools.get("forge_subscribe").execute("call", { target_record_ids: [41] })).details, { outcome: "not_configured" });
		assert.equal(requests.length, 2);
	} finally {
		await new Promise((resolve, reject) => server.close((error) => error ? reject(error) : resolve()));
		await rm(root, { recursive: true, force: true });
		for (const name of environmentNames) delete process.env[name];
	}
});

test("returns typed authorization and expiry outcomes but rejects malformed RPC responses", async () => {
	const root = await mkdtemp(join(tmpdir(), "forge-subscriptions-test-"));
	const secretFile = join(root, "capability");
	let reply = { error: { code: -32001, message: "denied", data: "assignment-capability-denied" } };
	const server = createServer(async (request, response) => {
		let raw = "";
		for await (const chunk of request) raw += chunk;
		const body = JSON.parse(raw);
		response.setHeader("content-type", "application/json");
		response.end(JSON.stringify({ jsonrpc: "2.0", id: body.id, ...reply }));
	});
	await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
	const address = server.address();
	try {
		await writeFile(secretFile, "secret");
		Object.assign(process.env, {
			FORGE_ASSIGNMENT_ID: "assignment-1",
			FORGE_ASSIGNMENT_DAEMON_URL: `http://127.0.0.1:${address.port}`,
			FORGE_ASSIGNMENT_CAPABILITY_SECRET_FILE: secretFile,
		});
		const { tools } = install();
		assert.deepEqual((await tools.get("forge_renew_subscription").execute("call", { subscription_id: 9 })).details, { outcome: "authorization_denied" });
		reply = { error: { code: -32001, message: "expired", data: "subscription-expired" } };
		assert.deepEqual((await tools.get("forge_renew_subscription").execute("call", { subscription_id: 9 })).details, { outcome: "subscription_expired" });
		reply = { error: { code: -32001, message: "deleted", data: "subscription-not-found" } };
		assert.deepEqual((await tools.get("forge_delete_subscription").execute("call", { subscription_id: 9 })).details, { outcome: "subscription_deleted" });
		reply = { result: "not-a-subscription" };
		await assert.rejects(() => tools.get("forge_renew_subscription").execute("call", { subscription_id: 9 }), /invalid subscription/);
	} finally {
		await new Promise((resolve, reject) => server.close((error) => error ? reject(error) : resolve()));
		await rm(root, { recursive: true, force: true });
		for (const name of environmentNames) delete process.env[name];
	}
});
