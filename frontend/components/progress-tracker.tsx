"use client";

import { useState, useEffect, useRef } from "react";
import { useSearchParams } from "next/navigation";
import {
	Play,
	Pause,
	CheckCircle,
	AlertCircle,
	Clock,
	Terminal,
	Square,
	Trash2,
	Download,
} from "lucide-react";
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
	getWipeJob,
	getWipeJobs,
} from "@/lib/utils";
import type { WipeJobRecord } from "@/lib/types";

interface WipeJob {
	id: string;
	deviceName: string;
	deviceModel: string;
	method: string;
	status: "running" | "paused" | "verifying" | "verified" | "failed" | "queued";
	progress: number;
	currentPass: number;
	totalPasses: number;
	startTime: string;
	estimatedCompletion: string;
	speed: string;
	evidenceHash: string;
	failure: string | null;
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
		status: record.status,
		progress:
			record.status === "verifying" || record.status === "verified" ? 100 : 0,
		currentPass: 0,
		totalPasses: 0,
		startTime: record.startedAt,
		estimatedCompletion: "",
		speed: "0 MB/s",
		evidenceHash: record.evidenceHash,
		failure: record.failure,
	};
}

export function ProgressTracker() {
	const [jobs, setJobs] = useState<Map<string, WipeJob>>(new Map());
	const [logs, setLogs] = useState<LogEntry[]>([]);
	const [selectedJobId, setSelectedJobId] = useState<string | null>(null);
	const [certificateMessage, setCertificateMessage] = useState<string | null>(
		null,
	);
	const ws = useRef<WebSocket | null>(null);
	const searchParams = useSearchParams();

	useEffect(() => {
		let cancelled = false;
		const requestedJobId = searchParams.get("jobId");
		getWipeJobs()
			.then(async (records) => {
				if (
					requestedJobId &&
					!records.some((record) => record.id === requestedJobId)
				) {
					records = [await getWipeJob(requestedJobId), ...records];
				}
				if (cancelled) return;
				setJobs(
					new Map(
						records.map((record) => [record.id, jobView(record)]),
					),
				);
				setSelectedJobId(
					requestedJobId || records.at(0)?.id || null,
				);
			})
			.catch((error) => {
				if (!cancelled) console.error("Failed to load wipe jobs:", error);
			});
		return () => {
			cancelled = true;
		};
	}, [searchParams]);

	useEffect(() => {
		ws.current = new WebSocket("ws://localhost:8080/ws");

		ws.current.onopen = () => console.log("WebSocket connected");
		ws.current.onclose = () => console.log("WebSocket disconnected");

		ws.current.onmessage = (event) => {
			try {
				const data = JSON.parse(event.data);

				if (data.jobId) {
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
								method:
									data.methodName || data.method || job.method,
								evidenceHash:
									data.evidenceHash || job.evidenceHash,
								failure: data.error || job.failure,
							};
							newJobs.set(data.jobId, updatedJob);
						}
						return newJobs;
					});
					if (
						data.status === "verified" ||
						data.status === "failed"
					) {
						getWipeJob(data.jobId)
							.then((record) => {
								setJobs((previous) => {
									const updated = new Map(previous);
									updated.set(record.id, jobView(record));
									return updated;
								});
							})
							.catch((error) =>
								console.error("Failed to refresh wipe job:", error),
							);
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
				setLogs((prev) => [...prev, newLog]);
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
				setLogs((prev) => [...prev, newLog]);
			}
		};

		return () => {
			ws.current?.close();
		};
	}, []);

	const activeJobs = Array.from(jobs.values());
	const selectedJobData = selectedJobId ? jobs.get(selectedJobId) : null;

	// ... (rest of the component remains the same, using activeJobs and selectedJobData)

	const getStatusIcon = (status: WipeJob["status"]) => {
		switch (status) {
			case "running":
				return <Play className="h-4 w-4 text-warning" />;
			case "paused":
				return <Pause className="h-4 w-4 text-muted-foreground" />;
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
			case "paused":
				return "bg-muted text-muted-foreground";
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
		try {
			await abortWipe(jobId);
			// Optionally, update job status locally for immediate feedback
		} catch (error) {
			console.error("Failed to abort wipe:", error);
			// TODO: Show error toast
		}
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

	return (
		<div className="space-y-6">
			<div>
				<h1 className="text-2xl font-bold text-foreground">
					Wipe Progress
				</h1>
				<p className="text-muted-foreground">
					Monitor active and completed data destruction operations
				</p>
			</div>

			<div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
				{/* Jobs Overview */}
				<div className="lg:col-span-2 space-y-4">
					<Card className="component-border component-border-hover">
						<CardHeader>
							<CardTitle className="flex items-center space-x-2">
								<Clock className="h-5 w-5" />
								<span>Active Operations</span>
							</CardTitle>
							<CardDescription>
								{activeJobs.length === 0
									? "No active or recent wipe operations."
									: "Current and recent wipe operations"}
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
								onClick={() => {
									setSelectedJobId(job.id);
									setCertificateMessage(null);
								}}
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
													{job.status ===
														"running" && (
														<Button
															variant="destructive"
															size="sm"
															onClick={() =>
																handleAbortWipe(
																	job.id,
																)
															}
														>
															<Square className="h-4 w-4 mr-2" />
															Abort
														</Button>
													)}
												</div>{" "}
											</div>

											<div className="space-y-2">
												<div className="flex justify-between text-sm">
													<span className="text-muted-foreground">
														Progress
													</span>
													<span className="font-medium">
														{Math.round(
															job.progress,
														)}
														%
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
														{job.method}
													</span>
												</div>
												<div>
													<span className="text-muted-foreground">
														Pass:
													</span>
													<span className="ml-2 font-medium">
														{job.currentPass} of{" "}
														{job.totalPasses}
													</span>
												</div>
												<div>
													<span className="text-muted-foreground">
														Started:
													</span>
													<span className="ml-2 font-medium">
														{formatTime(
															job.startTime,
														)}
													</span>
												</div>
												<div>
													<span className="text-muted-foreground">
														Speed:
													</span>
													<span className="ml-2 font-medium">
														{job.speed}
													</span>
												</div>
												<div>
													<span className="text-muted-foreground">
														ETA:
													</span>
													<span className="ml-2 font-medium">
														{
															job.estimatedCompletion
														}
													</span>
												</div>{" "}
											</div>
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
												{selectedJobData.method}
											</span>
										</div>
										<div className="flex justify-between">
											<span className="text-muted-foreground">
												Progress
											</span>
											<span className="font-medium">
												{Math.round(
													selectedJobData.progress,
												)}
												%
											</span>
										</div>
										{selectedJobData.evidenceHash && (
											<div className="space-y-1">
												<span className="text-muted-foreground">
													Evidence hash
												</span>
												<p className="break-all font-mono text-xs">
													{selectedJobData.evidenceHash}
												</p>
											</div>
										)}
										{selectedJobData.failure && (
											<p className="text-sm text-destructive">
												{selectedJobData.failure}
											</p>
										)}
									</div>

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

			{/* Log Viewer */}
			<Card className="component-border component-border-hover">
				<CardHeader>
					<div className="flex items-center justify-between">
						<div>
							<CardTitle className="flex items-center space-x-2">
								<Terminal className="h-5 w-5" />
								<span>Live Logs</span>
							</CardTitle>
							<CardDescription>
								Real-time operation logs and system messages
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
