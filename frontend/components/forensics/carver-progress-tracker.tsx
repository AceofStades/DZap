"use client";

import React from "react";
import {
	Activity,
	Gauge,
	FileSearch,
	AlertTriangle,
	StopCircle,
	CheckCircle2,
	Clock,
	HardDrive,
	Binary,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent } from "@/components/ui/card";
import type { CarveProgress } from "@/lib/types";

interface CarverProgressTrackerProps {
	progress: CarveProgress | null;
	onStop: () => void;
	isStopping: boolean;
}

export function CarverProgressTracker({
	progress,
	onStop,
	isStopping,
}: CarverProgressTrackerProps) {
	if (!progress) return null;

	const percent =
		progress.total_bytes > 0
			? Math.min(100, Math.round((progress.scanned_bytes / progress.total_bytes) * 1000) / 10)
			: 0;

	const formatBytes = (bytes: number): string => {
		if (bytes === 0) return "0 B";
		const k = 1024;
		const sizes = ["B", "KB", "MB", "GB", "TB"];
		const i = Math.floor(Math.log(bytes) / Math.log(k));
		return `${parseFloat((bytes / Math.pow(k, i)).toFixed(2))} ${sizes[i]}`;
	};

	const formatDuration = (secs: number): string => {
		const m = Math.floor(secs / 60);
		const s = secs % 60;
		return `${m}m ${s < 10 ? "0" : ""}${s}s`;
	};

	const isCompleted = progress.status === "completed";
	const isStopped = progress.status === "stopped";
	const isScanning = progress.status === "scanning";

	return (
		<Card className="border-border/80 bg-card/70 backdrop-blur-md shadow-md overflow-hidden">
			<div className="p-5 flex flex-col gap-4">
				{/* Top Status & Controls */}
				<div className="flex flex-wrap items-center justify-between gap-3 border-b border-border/50 pb-4">
					<div className="flex items-center space-x-3">
						<div
							className={`p-2 rounded-lg ${
								isScanning
									? "bg-emerald-500/15 text-emerald-600 dark:text-emerald-400 animate-pulse"
									: isCompleted
									? "bg-blue-500/15 text-blue-600 dark:text-blue-400"
									: "bg-amber-500/15 text-amber-600 dark:text-amber-400"
							}`}
						>
							<Activity className="h-5 w-5" />
						</div>
						<div>
							<div className="flex items-center space-x-2">
								<h3 className="text-sm font-bold text-foreground">
									{isScanning
										? "Streaming Block-Level Forensic Scan"
										: isCompleted
										? "Carving Execution Finished"
										: "Carving Scan Halted"}
								</h3>
								<Badge
									variant="outline"
									className={`text-xs font-mono capitalize ${
										isScanning
											? "border-emerald-500/40 text-emerald-600 dark:text-emerald-400 bg-emerald-500/10"
											: isCompleted
											? "border-blue-500/40 text-blue-600 dark:text-blue-400 bg-blue-500/10"
											: "border-amber-500/40 text-amber-600 dark:text-amber-400 bg-amber-500/10"
									}`}
								>
									{progress.status}
								</Badge>
							</div>
							<p className="text-xs text-muted-foreground font-mono mt-0.5">
								Target: {progress.device_path} | Task ID: {progress.task_id}
							</p>
						</div>
					</div>

					{/* Stop / Cancel Action */}
					{isScanning && (
						<Button
							variant="destructive"
							size="sm"
							onClick={onStop}
							disabled={isStopping}
							className="text-xs font-semibold flex items-center space-x-1.5 shadow-sm"
						>
							<StopCircle className="h-4 w-4" />
							<span>{isStopping ? "Stopping..." : "Stop & Finalize Evidence"}</span>
						</Button>
					)}
				</div>

				{/* Animated Progress Bar */}
				<div className="flex flex-col gap-1.5">
					<div className="flex justify-between text-xs font-mono">
						<span className="text-muted-foreground">
							Sector Range: <strong className="text-foreground">0 — {progress.total_sectors.toLocaleString()}</strong>
						</span>
						<span className="font-bold text-emerald-600 dark:text-emerald-400">
							{percent}% ({formatBytes(progress.scanned_bytes)} / {formatBytes(progress.total_bytes)})
						</span>
					</div>

					<div className="h-3 w-full bg-secondary/60 rounded-full overflow-hidden p-0.5 border border-border/40">
						<div
							className={`h-full rounded-full transition-all duration-300 ${
								isScanning
									? "bg-gradient-to-r from-emerald-500 to-teal-400 animate-pulse"
									: isCompleted
									? "bg-blue-500"
									: "bg-amber-500"
							}`}
							style={{ width: `${Math.max(1, percent)}%` }}
						/>
					</div>
				</div>

				{/* 5-Column Live Metrics Grid */}
				<div className="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-5 gap-3 pt-1">
					{/* 1. LBA Sector */}
					<div className="p-3 rounded-lg border border-border/60 bg-background/50 flex flex-col justify-between">
						<div className="flex items-center justify-between text-muted-foreground mb-1">
							<span className="text-[11px] font-medium">Current LBA</span>
							<Binary className="h-3.5 w-3.5 text-muted-foreground" />
						</div>
						<div className="font-mono text-sm font-bold text-foreground">
							#{progress.current_sector.toLocaleString()}
						</div>
						<span className="text-[10px] text-muted-foreground mt-0.5">Aligned cluster boundary</span>
					</div>

					{/* 2. Speed */}
					<div className="p-3 rounded-lg border border-border/60 bg-background/50 flex flex-col justify-between">
						<div className="flex items-center justify-between text-muted-foreground mb-1">
							<span className="text-[11px] font-medium">Throughput</span>
							<Gauge className="h-3.5 w-3.5 text-emerald-500" />
						</div>
						<div className="font-mono text-sm font-bold text-emerald-600 dark:text-emerald-400">
							{progress.speed_mbps} MB/s
						</div>
						<span className="text-[10px] text-muted-foreground mt-0.5">Streaming I/O window</span>
					</div>

					{/* 3. Artifacts Found */}
					<div className="p-3 rounded-lg border border-border/60 bg-background/50 flex flex-col justify-between">
						<div className="flex items-center justify-between text-muted-foreground mb-1">
							<span className="text-[11px] font-medium">Artifacts Carved</span>
							<FileSearch className="h-3.5 w-3.5 text-blue-500" />
						</div>
						<div className="font-mono text-sm font-bold text-foreground">
							{progress.artifacts_found}
							<span className="text-xs font-normal text-muted-foreground ml-1">
								({progress.valid_count} valid)
							</span>
						</div>
						<span className="text-[10px] text-muted-foreground mt-0.5">
							{progress.fragmented_count} reassembled
						</span>
					</div>

					{/* 4. Bad Sectors */}
					<div className="p-3 rounded-lg border border-border/60 bg-background/50 flex flex-col justify-between">
						<div className="flex items-center justify-between text-muted-foreground mb-1">
							<span className="text-[11px] font-medium">Bad Sectors</span>
							<AlertTriangle className={`h-3.5 w-3.5 ${progress.bad_sectors > 0 ? "text-rose-500" : "text-muted-foreground"}`} />
						</div>
						<div
							className={`font-mono text-sm font-bold ${
								progress.bad_sectors > 0 ? "text-rose-600 dark:text-rose-400" : "text-foreground"
							}`}
						>
							{progress.bad_sectors}
						</div>
						<span className="text-[10px] text-muted-foreground mt-0.5">
							{progress.bad_sectors > 0 ? "Bypassed with penalty" : "No I/O errors"}
						</span>
					</div>

					{/* 5. Elapsed Time */}
					<div className="p-3 rounded-lg border border-border/60 bg-background/50 flex flex-col justify-between">
						<div className="flex items-center justify-between text-muted-foreground mb-1">
							<span className="text-[11px] font-medium">Elapsed Time</span>
							<Clock className="h-3.5 w-3.5 text-muted-foreground" />
						</div>
						<div className="font-mono text-sm font-bold text-foreground">
							{formatDuration(progress.elapsed_secs)}
						</div>
						<span className="text-[10px] text-muted-foreground mt-0.5">Active investigation</span>
					</div>
				</div>
			</div>
		</Card>
	);
}
