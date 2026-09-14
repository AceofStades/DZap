"use client";

import { useCallback, useEffect, useState } from "react";
import { useRouter, useSearchParams } from "next/navigation";
import {
	AlertCircle,
	CheckCircle2,
	CirclePause,
	HardDriveDownload,
	Loader2,
	Play,
	RefreshCw,
	Square,
} from "lucide-react";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
	Card,
	CardContent,
	CardDescription,
	CardHeader,
	CardTitle,
} from "@/components/ui/card";
import { Progress } from "@/components/ui/progress";
import { RecoveryMethodPicker } from "@/components/recovery-method-picker";
import type { RecoveryJobRecord, RecoveryJobStatus } from "@/lib/types";
import {
	cancelRecoveryJob,
	getRecoveryJob,
	getRecoveryJobs,
	pauseRecoveryJob,
	resumeRecoveryJob,
} from "@/lib/utils";
import { cn } from "@/lib/utils";

const activeStatuses = new Set<RecoveryJobStatus>(["imaging", "extracting"]);

const statusLabels: Record<RecoveryJobStatus, string> = {
	imaging: "Imaging",
	paused: "Paused",
	image_complete: "Image complete",
	extracting: "Extracting files",
	completed: "Recovery complete",
	cancelled: "Cancelled",
	failed: "Failed",
};

function formatBytes(bytes: number) {
	if (!Number.isFinite(bytes) || bytes < 0) return "Unknown";
	const units = ["B", "KiB", "MiB", "GiB", "TiB"];
	const index =
		bytes === 0
			? 0
			: Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
	return `${(bytes / 1024 ** index).toFixed(index === 0 ? 0 : 2)} ${units[index]}`;
}

function statusStyle(status: RecoveryJobStatus) {
	if (status === "completed" || status === "image_complete") {
		return "bg-success/20 text-success";
	}
	if (status === "failed" || status === "cancelled") {
		return "bg-destructive/20 text-destructive";
	}
	return "bg-warning/20 text-warning";
}

function StatusIcon({ status }: { status: RecoveryJobStatus }) {
	if (status === "completed" || status === "image_complete") {
		return <CheckCircle2 className="h-5 w-5 text-success" />;
	}
	if (status === "failed" || status === "cancelled") {
		return <AlertCircle className="h-5 w-5 text-destructive" />;
	}
	if (status === "paused") {
		return <CirclePause className="h-5 w-5 text-warning" />;
	}
	return <HardDriveDownload className="h-5 w-5 text-warning" />;
}

export function RecoveryJobs() {
	const searchParams = useSearchParams();
	const router = useRouter();
	const requestedId = searchParams.get("recoveryJobId");
	const [jobs, setJobs] = useState<RecoveryJobRecord[]>([]);
	const [selectedId, setSelectedId] = useState<string | null>(requestedId);
	const [loading, setLoading] = useState(true);
	const [error, setError] = useState<string | null>(null);
	const [notice, setNotice] = useState<string | null>(null);
	const [pendingAction, setPendingAction] = useState<string | null>(null);

	const loadJobs = useCallback(async () => {
		try {
			let records = await getRecoveryJobs();
			if (requestedId && !records.some((record) => record.id === requestedId)) {
				records = [await getRecoveryJob(requestedId), ...records];
			}
			setJobs(records);
			setSelectedId((current) => {
				if (requestedId && records.some((record) => record.id === requestedId)) {
					return requestedId;
				}
				if (current && records.some((record) => record.id === current)) {
					return current;
				}
				return records.at(0)?.id ?? null;
			});
			setError(null);
		} catch (loadError) {
			setError(
				loadError instanceof Error
					? loadError.message
					: "Failed to load recovery jobs.",
			);
		} finally {
			setLoading(false);
		}
	}, [requestedId]);

	useEffect(() => {
		void loadJobs();
	}, [loadJobs]);

	useEffect(() => {
		if (!jobs.some((job) => activeStatuses.has(job.status))) return;
		const timer = window.setInterval(() => void loadJobs(), 2_000);
		return () => window.clearInterval(timer);
	}, [jobs, loadJobs]);

	const selectJob = (id: string) => {
		setSelectedId(id);
		setNotice(null);
		const next = new URLSearchParams(searchParams.toString());
		next.set("tab", "recovery");
		next.set("recoveryJobId", id);
		router.replace(`/?${next.toString()}`, { scroll: false });
	};

	const runAction = async (
		job: RecoveryJobRecord,
		action: "pause" | "cancel" | "resume",
	) => {
		setPendingAction(`${job.id}:${action}`);
		setError(null);
		setNotice(null);
		try {
			if (action === "pause") await pauseRecoveryJob(job.id);
			if (action === "cancel") await cancelRecoveryJob(job.id);
			if (action === "resume") await resumeRecoveryJob(job.id);
			setNotice(
				action === "resume"
					? "Resume accepted. DZap is continuing from the existing map file."
					: `${action === "pause" ? "Pause" : "Stop"} requested. DZap is waiting for ddrescue to flush its map file.`,
			);
			window.setTimeout(() => void loadJobs(), 350);
		} catch (actionError) {
			setError(
				actionError instanceof Error
					? actionError.message
					: `Failed to ${action} the recovery job.`,
			);
		} finally {
			setPendingAction(null);
		}
	};

	const selected = jobs.find((job) => job.id === selectedId) ?? null;
	const selectedHasCompleteImage = selected?.events.some(
		(event) => event.eventType === "image_completed",
	) ?? false;

	return (
		<div className="space-y-6">
			<div className="flex flex-wrap items-start justify-between gap-3">
				<div>
					<h1 className="text-2xl font-bold">Recovery</h1>
					<p className="text-muted-foreground">
						Monitor source imaging and resume safely from persistent map files.
					</p>
				</div>
				<Button
					variant="outline"
					size="sm"
					onClick={() => void loadJobs()}
					disabled={loading}
				>
					{loading ? (
						<Loader2 className="mr-2 h-4 w-4 animate-spin" />
					) : (
						<RefreshCw className="mr-2 h-4 w-4" />
					)}
					Reload records
				</Button>
			</div>

			{error && (
				<Alert variant="destructive">
					<AlertCircle className="h-4 w-4" />
					<AlertDescription>{error}</AlertDescription>
				</Alert>
			)}
			{notice && (
				<Alert>
					<AlertDescription>{notice}</AlertDescription>
				</Alert>
			)}

			{!loading && jobs.length === 0 ? (
				<Card>
					<CardContent className="py-10 text-center text-muted-foreground">
						No recovery jobs yet. Select a device, run its recovery
						assessment, and build an image plan to begin.
					</CardContent>
				</Card>
			) : (
				<div className="grid gap-6 lg:grid-cols-[minmax(18rem,0.8fr)_minmax(0,2fr)]">
					<div className="space-y-3">
						{jobs.map((job) => (
							<button
								type="button"
								key={job.id}
								onClick={() => selectJob(job.id)}
								className={cn(
									"w-full rounded-lg border bg-card p-4 text-left transition-colors hover:bg-accent/40",
									selectedId === job.id && "border-primary ring-1 ring-primary",
								)}
							>
								<div className="flex items-center justify-between gap-3">
									<div className="flex min-w-0 items-center gap-2">
										<StatusIcon status={job.status} />
										<span className="truncate font-medium">
											{job.sourceDevicePath}
										</span>
									</div>
									<Badge className={statusStyle(job.status)}>
										{statusLabels[job.status]}
									</Badge>
								</div>
								<Progress value={job.progressPercent} className="mt-3 h-2" />
								<p className="mt-2 text-xs text-muted-foreground">
									{Math.round(job.progressPercent)}% handled · updated{" "}
									{new Date(job.updatedAt).toLocaleString()}
								</p>
							</button>
						))}
					</div>

					{selected && (
						<Card>
							<CardHeader>
								<div className="flex flex-wrap items-start justify-between gap-3">
									<div>
										<CardTitle className="flex items-center gap-2">
											<StatusIcon status={selected.status} />
											{statusLabels[selected.status]}
										</CardTitle>
										<CardDescription className="mt-1 break-all">
											{selected.id}
										</CardDescription>
									</div>
									<div className="flex flex-wrap gap-2">
										{selected.status === "imaging" && (
											<>
												<Button
													variant="outline"
													onClick={() => void runAction(selected, "pause")}
													disabled={pendingAction !== null}
												>
													<CirclePause className="mr-2 h-4 w-4" />
													Pause safely
												</Button>
												<Button
													variant="destructive"
													onClick={() => void runAction(selected, "cancel")}
													disabled={pendingAction !== null}
												>
													<Square className="mr-2 h-4 w-4" />
													Stop and retain files
												</Button>
											</>
										)}
										{selected.status === "extracting" && (
											<Button
												variant="destructive"
												onClick={() => void runAction(selected, "cancel")}
												disabled={pendingAction !== null}
											>
												<Square className="mr-2 h-4 w-4" />
												Stop and retain files
											</Button>
										)}
										{(selected.status === "paused" ||
											(selected.status === "failed" &&
												!selectedHasCompleteImage)) && (
											<Button
												onClick={() => void runAction(selected, "resume")}
												disabled={pendingAction !== null}
											>
												<Play className="mr-2 h-4 w-4" />
												Resume from map
											</Button>
										)}
									</div>
								</div>
							</CardHeader>
							<CardContent className="space-y-5">
								<div>
									<div className="mb-2 flex justify-between text-sm">
										<span>Source ranges handled</span>
										<span className="font-medium">
											{selected.progressPercent.toFixed(1)}%
										</span>
									</div>
									<Progress value={selected.progressPercent} className="h-3" />
								</div>

								<div className="grid gap-3 sm:grid-cols-3">
									<div className="rounded-md bg-success/10 p-3">
										<p className="text-xs text-muted-foreground">Rescued</p>
										<p className="font-semibold text-success">
											{formatBytes(selected.mapSummary.rescuedBytes)}
										</p>
									</div>
									<div className="rounded-md bg-destructive/10 p-3">
										<p className="text-xs text-muted-foreground">Unreadable</p>
										<p className="font-semibold text-destructive">
											{formatBytes(selected.mapSummary.unreadableBytes)}
										</p>
									</div>
									<div className="rounded-md bg-muted p-3">
										<p className="text-xs text-muted-foreground">Pending</p>
										<p className="font-semibold">
											{formatBytes(selected.mapSummary.pendingBytes)}
										</p>
									</div>
								</div>

								<Alert>
									<AlertDescription>{selected.lastMessage}</AlertDescription>
								</Alert>

								{selected.recoveryResult && (
									<div className="rounded-md border border-success/40 bg-success/5 p-3 text-sm">
										<p className="font-medium text-success">
											Recovered {selected.recoveryResult.recoveredFileCount} files ({formatBytes(selected.recoveryResult.recoveredBytes)})
										</p>
										<p className="mt-1 break-all text-xs text-muted-foreground">
											Output: {selected.recoveryResult.outputDirectory}
										</p>
										<p className="mt-1 break-all font-mono text-xs text-muted-foreground">
											Manifest SHA-256: {selected.recoveryResult.manifestSha256}
										</p>
									</div>
								)}

								{selectedHasCompleteImage &&
									["image_complete", "completed", "cancelled", "failed"].includes(selected.status) && (
										<RecoveryMethodPicker job={selected} onChanged={loadJobs} />
									)}

								<div className="grid gap-3 text-sm sm:grid-cols-2">
									<div>
										<p className="text-xs text-muted-foreground">Source</p>
										<p className="break-all font-medium">{selected.sourceDevicePath}</p>
									</div>
									<div>
										<p className="text-xs text-muted-foreground">Destination</p>
										<p className="break-all font-medium">{selected.destination.devicePath}</p>
									</div>
									<div className="sm:col-span-2">
										<p className="text-xs text-muted-foreground">Image file</p>
										<p className="break-all font-mono text-xs">{selected.imagePath}</p>
									</div>
									<div className="sm:col-span-2">
										<p className="text-xs text-muted-foreground">Map file</p>
										<p className="break-all font-mono text-xs">{selected.mapPath}</p>
									</div>
									<div className="sm:col-span-2">
										<p className="text-xs text-muted-foreground">Evidence hash</p>
										<p className="break-all font-mono text-xs">{selected.evidenceHash}</p>
									</div>
								</div>
							</CardContent>
						</Card>
					)}
				</div>
			)}
		</div>
	);
}
