"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { useRouter, useSearchParams } from "next/navigation";
import {
	Play,
	CheckCircle,
	AlertCircle,
	Clock,
	Terminal,
	Square,
	Trash2,
	Download,
	Loader2,
	RefreshCw,
	Wifi,
	WifiOff,
} from "lucide-react";
import { Alert, AlertDescription } from "./ui/alert";
import { Button } from "./ui/button";
import {
	Card,
	CardContent,
	CardDescription,
	CardHeader,
	CardTitle,
} from "./ui/card";
import { Badge } from "./ui/badge";
import { Progress } from "./ui/progress";
import { Separator } from "./ui/separator";
import { ScrollArea } from "./ui/scroll-area";
import { cn } from "@/lib/utils";
import {
	abortWipe,
	generateCertificate,
	getWebSocketUrl,
	getWipeJob,
	getWipeJobs,
} from "@/lib/utils";
import type { WipeJobRecord } from "@/lib/types";

interface WipeJob {
	id: string;
	deviceName: string;
	deviceModel: string;
	method: string;
	methodName: string;
	status: "running" | "verifying" | "verified" | "failed";
	progress: number;
	currentPass: number;
	totalPasses: number;
	startTime: string;
	estimatedCompletion: string;
	speed: string;
	evidenceHash: string;
	failure: string | null;
}

type ConnectionState = "connecting" | "connected" | "reconnecting";

const MAX_RECONNECT_DELAY_MS = 10_000;
const MAX_LOG_ENTRIES = 500;

function isTerminalStatus(status: unknown) {
	return status === "verified" || status === "failed";
}

function isHostOverwrite(method: string) {
	return method.startsWith("overwrite_");
}

function methodLabel(method: string) {
	switch (method) {
		case "nvme_format":
			return "Purge: NVMe Format";
		case "nvme_sanitize_crypto":
			return "Purge: NVMe Sanitize (Crypto Erase)";
		case "nvme_sanitize_block":
			return "Purge: NVMe Sanitize (Block Erase)";
		case "nvme_sanitize_overwrite":
			return "Purge: NVMe Sanitize (Overwrite)";
		case "sata_secure_erase":
			return "Purge: ATA Secure Erase";
		case "sata_secure_erase_enhanced":
			return "Purge: ATA Enhanced Secure Erase";
		case "overwrite_1_pass":
			return "Clear: 1-Pass Overwrite";
		case "overwrite_2_pass":
			return "Clear: 2-Pass Overwrite";
		case "overwrite_3_pass":
			return "Purge: 3-Pass Overwrite";
		default:
			return method;
	}
}

interface LogEntry {
	id: string;
	timestamp: string;
	level: "info" | "warning" | "error" | "success";
	message: string;
	deviceId?: string;
}

function jobView(record: WipeJobRecord): WipeJob {
	return {
		id: record.id,
		deviceName: record.devicePath,
		deviceModel: record.deviceModel,
		method: record.method,
		methodName: methodLabel(record.method),
		status: record.status,
		progress:
			record.status === "verifying" || record.status === "verified" ? 100 : 0,
		currentPass: 0,
		totalPasses: 0,
		startTime: record.startedAt,
		estimatedCompletion: "",
		speed: "",
		evidenceHash: record.evidenceHash,
		failure: record.failure,
	};
}

export function ProgressTracker() {
	const [jobs, setJobs] = useState<Map<string, WipeJob>>(new Map());
	const [logs, setLogs] = useState<LogEntry[]>([]);
	const [selectedJobId, setSelectedJobId] = useState<string | null>(null);
	const [jobsLoading, setJobsLoading] = useState(true);
	const [jobsError, setJobsError] = useState<string | null>(null);
	const [connectionState, setConnectionState] =
		useState<ConnectionState>("connecting");
	const [abortNotice, setAbortNotice] = useState<{
		jobId: string;
		message: string;
	} | null>(null);
	const [certificateMessage, setCertificateMessage] = useState<string | null>(
		null,
	);
	const ws = useRef<WebSocket | null>(null);
	const jobsRef = useRef<Map<string, WipeJob>>(new Map());
	const jobsLoadSequence = useRef(0);
	const pendingJobRefreshes = useRef<Set<string>>(new Set());
	const searchParams = useSearchParams();
	const router = useRouter();
	const requestedJobId = searchParams.get("jobId");
	const requestedJobIdRef = useRef(requestedJobId);
	requestedJobIdRef.current = requestedJobId;

	const loadJobs = useCallback(async (preferredJobId?: string | null) => {
		const sequence = ++jobsLoadSequence.current;
		setJobsLoading(true);
		setJobsError(null);
		try {
			let records = await getWipeJobs();
			let preferredJobError: string | null = null;
			if (
				preferredJobId &&
				!records.some((record) => record.id === preferredJobId)
			) {
				try {
					records = [await getWipeJob(preferredJobId), ...records];
				} catch (error) {
					preferredJobError =
						error instanceof Error
							? error.message
							: `Wipe job ${preferredJobId} could not be loaded.`;
				}
			}

			if (sequence !== jobsLoadSequence.current) return;
			const loadedJobs = new Map(
				records.map((record) => {
					const loaded = jobView(record);
					const current = jobsRef.current.get(record.id);
					if (record.status === "running" && current?.status === "running") {
						loaded.progress = current.progress;
						loaded.currentPass = current.currentPass;
						loaded.totalPasses = current.totalPasses;
						loaded.speed = current.speed;
						loaded.estimatedCompletion = current.estimatedCompletion;
					}
					return [record.id, loaded] as const;
				}),
			);
			jobsRef.current = loadedJobs;
			setJobs(loadedJobs);
			setSelectedJobId((current) => {
				if (preferredJobId && loadedJobs.has(preferredJobId)) {
					return preferredJobId;
				}
				if (current && loadedJobs.has(current)) return current;
				return records.at(0)?.id || null;
			});
			setJobsError(preferredJobError);
		} catch (error) {
			if (sequence !== jobsLoadSequence.current) return;
			setJobsError(
				error instanceof Error
					? error.message
					: "Failed to load authoritative wipe jobs.",
			);
		} finally {
			if (sequence === jobsLoadSequence.current) setJobsLoading(false);
		}
	}, []);

	const refreshJob = useCallback(async (jobId: string) => {
		if (pendingJobRefreshes.current.has(jobId)) return;
		pendingJobRefreshes.current.add(jobId);
		try {
			const record = await getWipeJob(jobId);
			setJobs((previous) => {
				const updated = new Map(previous);
				updated.set(record.id, jobView(record));
				jobsRef.current = updated;
				return updated;
			});
			setSelectedJobId((current) => current || record.id);
		} catch (error) {
			console.error(`Failed to refresh wipe job ${jobId}:`, error);
		} finally {
			pendingJobRefreshes.current.delete(jobId);
		}
	}, []);

	useEffect(() => {
		if (requestedJobId && jobsRef.current.has(requestedJobId)) {
			jobsLoadSequence.current += 1;
			setJobsLoading(false);
			setSelectedJobId(requestedJobId);
			setJobsError(null);
			return;
		}
		void loadJobs(requestedJobId);
	}, [loadJobs, requestedJobId]);

	useEffect(() => {
		let disposed = false;
		let reconnectAttempt = 0;
		let reconnectTimer: ReturnType<typeof setTimeout> | null = null;

		const appendLog = (entry: LogEntry) => {
			setLogs((previous) => [
				...previous.slice(-(MAX_LOG_ENTRIES - 1)),
				entry,
			]);
		};

		const handleMessage = (event: MessageEvent<string>) => {
			try {
				const data = JSON.parse(event.data);

				if (typeof data.jobId === "string") {
					const knownJob = jobsRef.current.get(data.jobId);
					if (!knownJob) void refreshJob(data.jobId);
					setJobs((prevJobs) => {
						const newJobs = new Map(prevJobs);
						const job = newJobs.get(data.jobId);
						if (job) {
							const status =
								data.status === "verified" ||
								data.status === "verifying" ||
								data.status === "failed"
									? data.status
									: "running";
							const updatedJob = {
								...job,
								status,
								progress:
									status === "verified" || status === "verifying"
										? 100
										: (data.progress ?? job.progress),
								currentPass: data.currentPass ?? job.currentPass,
								totalPasses: data.totalPasses ?? job.totalPasses,
								speed: data.speed ?? job.speed,
								estimatedCompletion:
									data.eta ?? job.estimatedCompletion,
								deviceModel:
									data.deviceModel || job.deviceModel,
								method: data.method || job.method,
								methodName: data.methodName || job.methodName,
								evidenceHash:
									data.evidenceHash || job.evidenceHash,
								failure: data.error || job.failure,
							};
							newJobs.set(data.jobId, updatedJob);
						}
						jobsRef.current = newJobs;
						return newJobs;
					});
					if (isTerminalStatus(data.status)) {
						setAbortNotice((current) =>
							current?.jobId === data.jobId ? null : current,
						);
						void refreshJob(data.jobId);
					}
				}

				const newLog: LogEntry = {
					id: `${Date.now()}-${Math.random()}`,
					timestamp: new Date().toISOString(),
					level:
						data.status === "failed"
							? "error"
							: data.status === "verified"
								? "success"
								: "info",
					message: data.message || data.error || event.data,
					deviceId: data.jobId,
				};
				appendLog(newLog);
			} catch (e) {
				// Message is not JSON, treat as plain text log
				const newLog: LogEntry = {
					id: Date.now().toString(),
					timestamp: new Date().toISOString(),
					level: event.data.startsWith("ERROR:")
						? "error"
						: event.data.startsWith("SUCCESS:")
							? "success"
							: "info",
					message: event.data,
				};
				appendLog(newLog);
			}
		};

		const connect = () => {
			if (disposed) return;
			setConnectionState(reconnectAttempt === 0 ? "connecting" : "reconnecting");

			let socket: WebSocket;
			try {
				socket = new WebSocket(getWebSocketUrl());
			} catch (error) {
				console.error("Failed to create WebSocket:", error);
				scheduleReconnect();
				return;
			}
			ws.current = socket;

			socket.onopen = () => {
				if (disposed) return;
				reconnectAttempt = 0;
				setConnectionState("connected");
				void loadJobs(requestedJobIdRef.current);
			};
			socket.onmessage = handleMessage;
			socket.onerror = () => socket.close();
			socket.onclose = () => {
				if (disposed) return;
				if (ws.current === socket) ws.current = null;
				scheduleReconnect();
			};
		};

		const scheduleReconnect = () => {
			if (disposed || reconnectTimer !== null) return;
			reconnectAttempt += 1;
			setConnectionState("reconnecting");
			const delay = Math.min(
				1_000 * 2 ** Math.min(reconnectAttempt - 1, 4),
				MAX_RECONNECT_DELAY_MS,
			);
			reconnectTimer = setTimeout(() => {
				reconnectTimer = null;
				connect();
			}, delay);
		};

		connect();

		return () => {
			disposed = true;
			if (reconnectTimer !== null) clearTimeout(reconnectTimer);
			const socket = ws.current;
			ws.current = null;
			socket?.close();
		};
	}, [loadJobs, refreshJob]);

	const activeJobs = Array.from(jobs.values());
	const selectedJobData = selectedJobId ? jobs.get(selectedJobId) : null;

	// ... (rest of the component remains the same, using activeJobs and selectedJobData)

	const getStatusIcon = (status: WipeJob["status"]) => {
		switch (status) {
			case "running":
				return <Play className="h-4 w-4 text-warning" />;
			case "verified":
				return <CheckCircle className="h-4 w-4 text-success" />;
			case "verifying":
				return <Clock className="h-4 w-4 text-warning" />;
			case "failed":
				return <AlertCircle className="h-4 w-4 text-destructive" />;
			default:
				return <Clock className="h-4 w-4 text-muted-foreground" />;
		}
	};

	const getStatusColor = (status: WipeJob["status"]) => {
		switch (status) {
			case "running":
				return "bg-warning/20 text-warning";
			case "verified":
				return "bg-success/20 text-success";
			case "verifying":
				return "bg-warning/20 text-warning";
			case "failed":
				return "bg-destructive/20 text-destructive";
			default:
				return "bg-muted/20 text-muted-foreground";
		}
	};

	const getLogLevelColor = (level: LogEntry["level"]) => {
		switch (level) {
			case "info":
				return "text-blue-400";
			case "warning":
				return "text-yellow-400";
			case "error":
				return "text-red-400";
			case "success":
				return "text-green-400";
		}
	};

	const formatTime = (isoString: string) => {
		return new Date(isoString).toLocaleTimeString();
	};

	const filteredLogs = selectedJobId
		? logs.filter((log) => log.deviceId === selectedJobId)
		: logs;

	const handleAbortWipe = async (jobId: string) => {
		const job = jobsRef.current.get(jobId);
		setAbortNotice({ jobId, message: "Requesting abort..." });
		try {
			await abortWipe(jobId);
			setAbortNotice({
				jobId,
				message:
					job && isHostOverwrite(job.method)
						? "Abort requested. The host overwrite will stop between write chunks; wait for the final failed job record."
						: "Abort requested. The local command will be stopped, but a drive may continue a firmware operation it already accepted; wait for the final job record.",
			});
		} catch (error) {
			setAbortNotice({
				jobId,
				message:
					error instanceof Error ? error.message : "Failed to request abort.",
			});
		}
	};

	const selectJob = (jobId: string) => {
		setSelectedJobId(jobId);
		setCertificateMessage(null);
		setAbortNotice(null);
		const next = new URLSearchParams(searchParams.toString());
		next.set("tab", "progress");
		next.set("jobId", jobId);
		router.replace(`/?${next.toString()}`, { scroll: false });
	};

	const handleGenerateCertificate = async (jobId: string) => {
		setCertificateMessage("Generating certificate...");
		try {
			await generateCertificate(jobId);
			setCertificateMessage(
				"Certificate generated. It is now available in Certificates.",
			);
		} catch (error) {
			setCertificateMessage(
				error instanceof Error
					? error.message
					: "Failed to generate certificate.",
			);
		}
	};

	const handleExportLogs = () => {
		const logData = filteredLogs.map((log) => ({
			timestamp: log.timestamp,
			level: log.level,
			message: log.message,
			deviceId: log.deviceId,
		}));

		const blob = new Blob([JSON.stringify(logData, null, 2)], {
			type: "application/json",
		});
		const url = URL.createObjectURL(blob);
		const a = document.createElement("a");
		a.href = url;
		a.download = `wipe-logs-${new Date().toISOString().split("T")[0]}.json`;
		document.body.appendChild(a);
		a.click();
		document.body.removeChild(a);
		URL.revokeObjectURL(url);
	};

	const handleClearLogs = () => {
		setLogs([]);
	};

	const connectionLabel =
		connectionState === "connected"
			? "Live updates connected"
			: connectionState === "connecting"
				? "Connecting to live updates"
				: "Reconnecting to live updates";

	return (
		<div className="space-y-6">
			<div className="flex flex-wrap items-start justify-between gap-3">
				<div>
					<h1 className="text-2xl font-bold text-foreground">
						Wipe Progress
					</h1>
					<p className="text-muted-foreground">
						Monitor server-owned data destruction records
					</p>
				</div>
				<div className="flex flex-wrap items-center gap-2">
					<Badge
						variant="outline"
						className={cn(
							"gap-1.5",
							connectionState === "connected"
								? "border-success/50 text-success"
								: "border-warning/50 text-warning",
						)}
					>
						{connectionState === "connected" ? (
							<Wifi className="h-3.5 w-3.5" />
						) : (
							<WifiOff className="h-3.5 w-3.5" />
						)}
						{connectionLabel}
					</Badge>
					<Button
						variant="outline"
						size="sm"
						onClick={() => void loadJobs(selectedJobId)}
						disabled={jobsLoading}
					>
						{jobsLoading ? (
							<Loader2 className="mr-2 h-4 w-4 animate-spin" />
						) : (
							<RefreshCw className="mr-2 h-4 w-4" />
						)}
						Reload records
					</Button>
				</div>
			</div>

			{jobsError && (
				<Alert variant="destructive">
					<AlertCircle className="h-4 w-4" />
					<AlertDescription>{jobsError}</AlertDescription>
				</Alert>
			)}

			<div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
				<div className="lg:col-span-2 space-y-4">
					<Card className="component-border component-border-hover">
						<CardHeader>
							<CardTitle className="flex items-center space-x-2">
								<Clock className="h-5 w-5" />
								<span>Operation History</span>
							</CardTitle>
							<CardDescription>
								{jobsLoading && activeJobs.length === 0
									? "Loading authoritative wipe records..."
									: activeJobs.length === 0
									? "No active or recent wipe operations."
									: `${activeJobs.length} persisted wipe record${activeJobs.length === 1 ? "" : "s"}`}
							</CardDescription>
						</CardHeader>
						<CardContent className="space-y-4">
							{activeJobs.map((job) => (
								<Card
									key={job.id}
									className={cn(
										"transition-all duration-200 hover:shadow-md component-border component-border-hover",
										selectedJobId === job.id &&
											"ring-2 ring-primary",
									)}
									onClick={() => selectJob(job.id)}
								>
									<CardContent className="p-4">
										<div className="space-y-3">
											<div className="flex items-center justify-between">
												<div className="flex items-center space-x-3">
													{getStatusIcon(job.status)}
													<div>
														<h4 className="font-medium text-foreground">
															{job.deviceName}
														</h4>
														<p className="text-sm text-muted-foreground">
															{job.deviceModel}
														</p>
													</div>
												</div>
												<div className="flex items-center space-x-2">
													<Badge
														className={getStatusColor(
															job.status,
														)}
													>
														{job.status.toUpperCase()}
													</Badge>
												{job.status === "running" && (
													<Button
														variant="destructive"
														size="sm"
														onClick={(event) => {
															event.stopPropagation();
															selectJob(job.id);
															void handleAbortWipe(job.id);
														}}
													>
														<Square className="mr-2 h-4 w-4" />
														{isHostOverwrite(job.method)
															? "Abort overwrite"
															: "Request abort"}
													</Button>
												)}
											</div>
											</div>

											<div className="space-y-2">
												<div className="flex justify-between text-sm">
													<span className="text-muted-foreground">
														Progress
													</span>
													<span className="font-medium">
														{job.status === "running" &&
														job.totalPasses === 0
															? "Awaiting live update"
															: `${Math.round(job.progress)}%`}
													</span>
												</div>
												<Progress
													value={job.progress}
													className="h-2"
												/>
											</div>

											<div className="grid grid-cols-2 gap-4 text-sm">
												<div>
													<span className="text-muted-foreground">
														Method:
													</span>
													<span className="ml-2 font-medium">
														{job.methodName}
													</span>
												</div>
												<div>
													<span className="text-muted-foreground">
														Started:
													</span>
													<span className="ml-2 font-medium">
														{formatTime(job.startTime)}
													</span>
												</div>
												{job.totalPasses > 0 && (
													<div>
														<span className="text-muted-foreground">
															Pass:
														</span>
														<span className="ml-2 font-medium">
															{job.currentPass} of {job.totalPasses}
														</span>
													</div>
												)}
												{job.speed && (
													<div>
														<span className="text-muted-foreground">
															Speed:
														</span>
														<span className="ml-2 font-medium">
															{job.speed}
														</span>
													</div>
												)}
												{job.estimatedCompletion && (
													<div>
														<span className="text-muted-foreground">
															ETA:
														</span>
														<span className="ml-2 font-medium">
															{job.estimatedCompletion}
														</span>
													</div>
												)}
											</div>
											{job.failure && (
												<Alert variant="destructive">
													<AlertDescription>
														{job.failure}
													</AlertDescription>
												</Alert>
											)}
										</div>
									</CardContent>
								</Card>
							))}
						</CardContent>
					</Card>
				</div>

				{/* Job Details */}
				<div className="space-y-4">
					<Card className="component-border component-border-hover">
						<CardHeader>
							<CardTitle className="flex items-center space-x-2">
								<Terminal className="h-5 w-5" />
								<span>Operation Details</span>
							</CardTitle>
							<CardDescription>
								Detailed information for selected operation
							</CardDescription>
						</CardHeader>
						<CardContent>
							{selectedJobData ? (
								<div className="space-y-4">
									<div className="space-y-2">
										<h4 className="font-medium text-foreground">
											{selectedJobData.deviceName}
										</h4>
										<p className="text-sm text-muted-foreground">
											{selectedJobData.deviceModel}
										</p>
									</div>

									<Separator />

									<div className="space-y-3 text-sm">
										<div className="flex justify-between">
											<span className="text-muted-foreground">
												Status
											</span>
											<Badge
												className={getStatusColor(
													selectedJobData.status,
												)}
											>
												{selectedJobData.status.toUpperCase()}
											</Badge>
										</div>
										<div className="flex justify-between">
											<span className="text-muted-foreground">
												Method
											</span>
											<span className="font-medium">
												{selectedJobData.methodName}
											</span>
										</div>
										<div className="flex justify-between">
											<span className="text-muted-foreground">
												Progress
											</span>
											<span className="font-medium">
												{selectedJobData.status === "running" &&
												selectedJobData.totalPasses === 0
													? "Awaiting live update"
													: `${Math.round(selectedJobData.progress)}%`}
											</span>
										</div>
										{selectedJobData.evidenceHash && (
											<div className="space-y-1">
												<span className="text-muted-foreground">
													Evidence-chain hash
												</span>
												<p className="break-all font-mono text-xs">
													{selectedJobData.evidenceHash}
												</p>
											</div>
										)}
										{selectedJobData.failure && (
											<Alert variant="destructive">
												<AlertDescription>
													{selectedJobData.failure}
												</AlertDescription>
											</Alert>
										)}
									</div>

									{selectedJobData.status === "running" && (
										<Alert>
											<AlertDescription>
												{isHostOverwrite(selectedJobData.method)
													? "Abort stops a host overwrite between write chunks. The server records the final outcome."
													: "A firmware operation may continue inside the drive after DZap stops its local command. The server record reports only what DZap can confirm."}
											</AlertDescription>
										</Alert>
									)}

									{abortNotice?.jobId === selectedJobData.id && (
										<p className="text-xs text-muted-foreground">
											{abortNotice.message}
										</p>
									)}

									{selectedJobData.status === "verified" && (
										<>
											<Separator />
											<Button
												className="w-full"
												onClick={() =>
													handleGenerateCertificate(
														selectedJobData.id,
													)
												}
											>
												Generate Certificate
											</Button>
											{certificateMessage && (
												<p className="text-xs text-muted-foreground">
													{certificateMessage}
												</p>
											)}
										</>
									)}
								</div>
							) : (
								<p className="text-muted-foreground text-sm">
									Select an operation to view details
								</p>
							)}
						</CardContent>
					</Card>
				</div>
			</div>

			<Card className="component-border component-border-hover">
				<CardHeader>
					<div className="flex items-center justify-between">
						<div>
							<CardTitle className="flex items-center space-x-2">
								<Terminal className="h-5 w-5" />
								<span>Live Logs</span>
							</CardTitle>
							<CardDescription>
								WebSocket messages observed during this dashboard session
							</CardDescription>
						</div>
						<div className="flex space-x-2">
							<Button
								variant="outline"
								size="sm"
								onClick={handleClearLogs}
							>
								<Trash2 className="h-4 w-4 mr-2" />
								Clear
							</Button>
							<Button
								variant="outline"
								size="sm"
								onClick={handleExportLogs}
							>
								<Download className="h-4 w-4 mr-2" />
								Export
							</Button>
						</div>
					</div>
				</CardHeader>
				<CardContent>
					<ScrollArea className="h-64 w-full bg-black rounded-md p-4 font-mono text-sm component-border">
						<div className="space-y-1">
							{filteredLogs.map((log) => (
								<div key={log.id} className="flex space-x-2">
									<span className="text-gray-500 shrink-0">
										[
										{new Date(
											log.timestamp,
										).toLocaleTimeString()}
										]
									</span>
									<span
										className={cn(
											"shrink-0 uppercase",
											getLogLevelColor(log.level),
										)}
									>
										[{log.level}]
									</span>
									<span className="text-green-400">
										{log.message}
									</span>
								</div>
							))}
						</div>
					</ScrollArea>
				</CardContent>
			</Card>
		</div>
	);
}
