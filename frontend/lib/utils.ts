import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";
import type {
	DeviceInventory,
	DriveHealth,
	EvidenceExportResult,
	ExportDestination,
	RecoveryAssessment,
	RecoveryDestination,
	RecoveryImagePlan,
	DeviceIdentity,
	SignedCertificate,
	StartWipeResponse,
	WipeJobRecord,
	WipePlan,
	WipeRequest,
	WipeMethod,
} from "@/lib/types";

export function cn(...inputs: ClassValue[]) {
	return twMerge(clsx(inputs));
}

const configuredServerOrigin =
	process.env.NEXT_PUBLIC_DZAP_SERVER_ORIGIN?.trim() || null;

function backendUrl(path: string): string {
	if (typeof window === "undefined") {
		throw new Error("The DZap backend URL is only available in the browser.");
	}
	const origin = configuredServerOrigin || window.location.origin;
	return new URL(path, `${origin.replace(/\/$/, "")}/`).toString();
}

function apiUrl(path: string): string {
	return backendUrl(`/api${path}`);
}

export function getWebSocketUrl(): string {
	const url = new URL(backendUrl("/ws"));
	url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
	return url.toString();
}

export async function getDevices(): Promise<DeviceInventory> {
	const response = await fetch(apiUrl("/drives"));
	if (!response.ok) {
		throw new Error("Failed to fetch devices");
	}
	return response.json();
}

export async function getDriveHealth(deviceName: string): Promise<DriveHealth> {
	const drive = deviceName.replace("/dev/", "");
	const response = await fetch(apiUrl(`/drive/${drive}/health`));
	if (!response.ok) {
		throw new Error("Failed to fetch drive health");
	}
	return response.json();
}

export async function assessRecovery(
	devicePath: string,
): Promise<RecoveryAssessment> {
	const response = await fetch(apiUrl("/recovery/assess"), {
		method: "POST",
		headers: {
			"Content-Type": "application/json",
		},
		body: JSON.stringify({ devicePath }),
	});
	if (!response.ok) {
		throw await apiResponseError(
			response,
			"Failed to assess the recovery source.",
		);
	}
	return response.json();
}

export async function getRecoveryDestinations(
	sourceDevicePath: string,
): Promise<RecoveryDestination[]> {
	const query = new URLSearchParams({ sourceDevicePath });
	const response = await fetch(apiUrl(`/recovery/destinations?${query}`));
	if (!response.ok) {
		throw await apiResponseError(
			response,
			"Failed to discover recovery destinations.",
		);
	}
	return response.json();
}

export async function mountRecoveryDestination(
	sourceDevicePath: string,
	expectedSourceIdentity: DeviceIdentity,
	destination: RecoveryDestination,
): Promise<RecoveryDestination> {
	const response = await fetch(apiUrl("/recovery/destinations/mount"), {
		method: "POST",
		headers: { "Content-Type": "application/json" },
		body: JSON.stringify({
			sourceDevicePath,
			expectedSourceIdentity,
			destination,
		}),
	});
	if (!response.ok) {
		throw await apiResponseError(
			response,
			"Failed to mount the recovery destination.",
		);
	}
	return response.json();
}

export async function planRecoveryImage(
	sourceDevicePath: string,
	expectedSourceIdentity: DeviceIdentity,
	destination: RecoveryDestination,
): Promise<RecoveryImagePlan> {
	const response = await fetch(apiUrl("/recovery/plan"), {
		method: "POST",
		headers: { "Content-Type": "application/json" },
		body: JSON.stringify({
			sourceDevicePath,
			expectedSourceIdentity,
			destination,
		}),
	});
	if (!response.ok) {
		throw await apiResponseError(
			response,
			"Failed to build the recovery image plan.",
		);
	}
	return response.json();
}

async function wipeRequestError(response: Response): Promise<Error> {
	const body = await response.json().catch(() => null);
	if (body?.decision === "blocked" && Array.isArray(body.checks)) {
		const reasons = body.checks
			.filter((check: { status?: string }) => check.status === "blocked")
			.map((check: { message?: string }) => check.message)
			.filter(Boolean);
		return new Error(reasons.join(" ") || "Wipe blocked by safety preflight.");
	}
	return new Error(body?.error || `Server error: ${response.status}`);
}

export async function preflightWipe(config: WipeRequest): Promise<WipePlan> {
	const response = await fetch(apiUrl("/wipe/preflight"), {
		method: "POST",
		headers: {
			"Content-Type": "application/json",
		},
		body: JSON.stringify(config),
	});
	if (!response.ok) {
		throw await wipeRequestError(response);
	}
	return response.json();
}

export async function startWipe(
	config: WipeRequest,
): Promise<StartWipeResponse> {
	const response = await fetch(apiUrl("/wipe"), {
		method: "POST",
		headers: {
			"Content-Type": "application/json",
		},
		body: JSON.stringify(config),
	});
	if (!response.ok) {
		throw await wipeRequestError(response);
	}
	return response.json();
}

export async function getWipeMethods(deviceId: string): Promise<WipeMethod[]> {
	// The ID for storage devices is `/dev/sda`, so we remove `/dev/`.
	// For mobile, it's the serial, which is what we want.
	const identifier = deviceId.startsWith("/dev/")
		? deviceId.substring(5)
		: deviceId;
	const response = await fetch(apiUrl(`/drive/${identifier}/wipe-methods`));
	if (!response.ok) {
		throw new Error("Failed to fetch wipe methods");
	}
	return response.json();
}

export async function getCertificates(): Promise<SignedCertificate[]> {
	const response = await fetch(apiUrl("/certificates"));
	if (!response.ok) {
		throw new Error("Failed to fetch certificates");
	}
	return response.json();
}

export async function getWipeJobs(): Promise<WipeJobRecord[]> {
	const response = await fetch(apiUrl("/wipe/jobs"));
	if (!response.ok) {
		throw await apiResponseError(response, "Failed to fetch wipe jobs.");
	}
	return response.json();
}

export async function getWipeJob(jobId: string): Promise<WipeJobRecord> {
	const response = await fetch(
		apiUrl(`/wipe/jobs/${encodeURIComponent(jobId)}`),
	);
	if (!response.ok) {
		throw await apiResponseError(response, `Failed to fetch wipe job ${jobId}.`);
	}
	return response.json();
}

export async function unmountDevice(devicePath: string) {
	const response = await fetch(apiUrl("/unmount"), {
		method: "POST",
		headers: {
			"Content-Type": "application/json",
		},
		body: JSON.stringify({ device: devicePath }),
	});
	if (!response.ok) {
		throw await apiResponseError(response, "Failed to unmount device.");
	}
	return response.json();
}

export async function pauseWipe(deviceId: string) {
	const response = await fetch(apiUrl("/wipe/pause"), {
		method: "POST",
		headers: {
			"Content-Type": "application/json",
		},
		body: JSON.stringify({ deviceId }),
	});
	if (!response.ok) {
		throw new Error("Failed to pause wipe");
	}
	return response.json();
}

export async function abortWipe(deviceId: string) {
	const response = await fetch(apiUrl("/wipe/abort"), {
		method: "POST",
		headers: {
			"Content-Type": "application/json",
		},
		body: JSON.stringify({ deviceId }),
	});
	if (!response.ok) {
		throw await apiResponseError(response, "Failed to request wipe abort.");
	}
	return response.json();
}

export async function generateCertificate(
	jobId: string,
): Promise<SignedCertificate> {
	const response = await fetch(apiUrl("/certificate/generate"), {
		method: "POST",
		headers: {
			"Content-Type": "application/json",
		},
		body: JSON.stringify({ jobId }),
	});
	if (!response.ok) {
		throw new Error("Failed to generate certificate");
	}
	return response.json();
}

export async function downloadCertificatePdf(jobId: string): Promise<Blob> {
	const response = await fetch(apiUrl("/certificate?format=pdf"), {
		method: "POST",
		headers: {
			"Content-Type": "application/json",
		},
		body: JSON.stringify({ jobId }),
	});
	if (!response.ok) {
		throw new Error("Failed to download certificate PDF");
	}
	return response.blob();
}

async function apiResponseError(
	response: Response,
	fallback: string,
): Promise<Error> {
	const body = await response.json().catch(() => null);
	return new Error(body?.error || fallback);
}

export async function getEvidenceDestinations(): Promise<ExportDestination[]> {
	const response = await fetch(apiUrl("/evidence/destinations"));
	if (!response.ok) {
		throw await apiResponseError(
			response,
			"Failed to discover evidence destinations.",
		);
	}
	return response.json();
}

export async function mountEvidenceDestination(
	destination: ExportDestination,
): Promise<ExportDestination> {
	const response = await fetch(apiUrl("/evidence/mount"), {
		method: "POST",
		headers: {
			"Content-Type": "application/json",
		},
		body: JSON.stringify({ destination }),
	});
	if (!response.ok) {
		throw await apiResponseError(
			response,
			"Failed to mount evidence destination.",
		);
	}
	return response.json();
}

export async function exportEvidence(
	jobId: string,
	destination: ExportDestination,
): Promise<EvidenceExportResult> {
	const response = await fetch(apiUrl("/evidence/export"), {
		method: "POST",
		headers: {
			"Content-Type": "application/json",
		},
		body: JSON.stringify({ jobId, destination }),
	});
	if (!response.ok) {
		throw await apiResponseError(
			response,
			"Failed to export evidence bundle.",
		);
	}
	return response.json();
}
