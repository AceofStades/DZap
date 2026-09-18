"use client";

import React, { useState, useEffect, useRef, useCallback } from "react";
import {
	Shield,
	Search,
	FolderArchive,
	FileCheck2,
	RefreshCw,
	SlidersHorizontal,
	Binary,
	Layers,
	HardDrive,
	Download,
	FileText,
	CheckCircle2,
	AlertCircle,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Tabs, TabsList, TabsTrigger, TabsContent } from "@/components/ui/tabs";
import { CarverConfigPanel } from "./carver-config-panel";
import { CarverProgressTracker } from "./carver-progress-tracker";
import { EvidenceGallery } from "./evidence-gallery";
import { ChainOfCustodyModal } from "./chain-of-custody-modal";
import type { StorageDevice, CarveTarget, CarveProgress, CarvedArtifact, ChainOfCustodyReport } from "@/lib/types";
import {
	getDevices,
	startCarve,
	stopCarve,
	getCarveStatus,
	getCarveArtifacts,
	exportCarveReport,
	getWebSocketUrl,
} from "@/lib/utils";

export function ForensicWorkspace() {
	const [devices, setDevices] = useState<StorageDevice[]>([]);
	const [selectedDevice, setSelectedDevice] = useState<StorageDevice | null>(null);
	const [progress, setProgress] = useState<CarveProgress | null>(null);
	const [artifacts, setArtifacts] = useState<CarvedArtifact[]>([]);
	const [isStopping, setIsStopping] = useState(false);
	const [report, setReport] = useState<ChainOfCustodyReport | null>(null);
	const [isReportModalOpen, setIsReportModalOpen] = useState(false);
	const [activeTab, setActiveTab] = useState<string>("scanner");
	const [errorBanner, setErrorBanner] = useState<string | null>(null);

	const ws = useRef<WebSocket | null>(null);
	const isScanning = progress?.status === "scanning";

	// 1. Fetch available physical drives
	const loadDevices = useCallback(async () => {
		try {
			const inventory = await getDevices();
			const storageList: StorageDevice[] = (inventory.storage || []).map((d) => ({
				...d,
				id: d.name,
				deviceCategory: "storage",
			}));
			setDevices(storageList);
			if (storageList.length > 0 && !selectedDevice) {
				setSelectedDevice(storageList[0]);
			}
		} catch (err) {
			console.error("Failed to load devices:", err);
		}
	}, [selectedDevice]);

	// 2. Poll initial / existing task status
	const loadInitialStatus = useCallback(async () => {
		try {
			const statusRes = await getCarveStatus();
			if (statusRes.progress) {
				setProgress(statusRes.progress);
			}
			const artRes = await getCarveArtifacts();
			if (artRes.artifacts) {
				setArtifacts(artRes.artifacts);
			}
		} catch (err) {
			console.error("Failed to load initial carving state:", err);
		}
	}, []);

	useEffect(() => {
		loadDevices();
		loadInitialStatus();
	}, [loadDevices, loadInitialStatus]);

	// 3. Connect to live WebSocket hub for real-time telemetry and carved artifact notifications
	useEffect(() => {
		let socket: WebSocket;
		try {
			socket = new WebSocket(getWebSocketUrl());
			ws.current = socket;

			socket.onopen = () => {
				console.log("[Forensic WS] Connected to DZap real-time telemetry channel");
			};

			socket.onmessage = (event) => {
				try {
					const data = JSON.parse(event.data);

					if (data.event === "carve_started") {
						setArtifacts([]);
						setErrorBanner(null);
					} else if (data.event === "carve_progress") {
						setProgress(data.progress);
					} else if (data.event === "carve_artifact_found") {
						setArtifacts((prev) => {
							// Avoid duplicates
							if (prev.some((a) => a.id === data.artifact.id)) return prev;
							return [data.artifact, ...prev];
						});
					} else if (data.event === "carve_completed") {
						if (data.progress) {
							setProgress(data.progress);
						}
					}
				} catch (e) {
					// Ignore non-carver messages
				}
			};

			socket.onerror = (err) => {
				console.warn("[Forensic WS] Error:", err);
			};
		} catch (err) {
			console.error("[Forensic WS] Initialization error:", err);
		}

		return () => {
			if (ws.current) {
				ws.current.close();
			}
		};
	}, []);

	// 4. Handle start carve
	const handleStartCarve = async (target: CarveTarget) => {
		try {
			setErrorBanner(null);
			await startCarve(target);
		} catch (err: any) {
			console.error("Carve start error:", err);
			setErrorBanner(err.message || "Failed to initialize forensic carving engine.");
		}
	};

	// 5. Handle stop carve
	const handleStopCarve = async () => {
		try {
			setIsStopping(true);
			await stopCarve();
		} catch (err) {
			console.error("Carve stop error:", err);
		} finally {
			setIsStopping(false);
		}
	};

	// 6. Handle generate Chain of Custody Report
	const handleOpenReport = async () => {
		try {
			const rep = await exportCarveReport();
			setReport(rep);
			setIsReportModalOpen(true);
		} catch (err: any) {
			setErrorBanner(err.message || "Failed to generate Chain of Custody report.");
		}
	};

	return (
		<div className="flex-1 flex flex-col h-full bg-background text-foreground overflow-y-auto px-6 py-6 max-w-7xl mx-auto w-full gap-6">
			{/* Error Banner */}
			{errorBanner && (
				<div className="p-3.5 rounded-xl bg-rose-500/10 border border-rose-500/30 text-rose-600 dark:text-rose-400 text-xs flex items-center justify-between shadow-sm">
					<div className="flex items-center gap-2">
						<AlertCircle className="h-4 w-4 shrink-0" />
						<span>{errorBanner}</span>
					</div>
					<Button variant="ghost" size="sm" onClick={() => setErrorBanner(null)} className="h-6 text-xs px-2">
						Dismiss
					</Button>
				</div>
			)}

			{/* Top Hero & Quick Stats */}
			<div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4 border-b border-border/70 pb-5">
				<div>
					<div className="flex items-center space-x-2.5">
						<h1 className="text-xl font-bold tracking-tight text-foreground">
							Forensic Media & File Carving Workspace
						</h1>
						<Badge
							variant="outline"
							className="border-emerald-500/30 text-emerald-600 dark:text-emerald-400 bg-emerald-500/10 text-xs font-mono"
						>
							SCALPEL & GARFINKEL HEURISTICS
						</Badge>
					</div>
					<p className="text-xs text-muted-foreground mt-1">
						Recover deleted, formatted, or fragmented files from raw storage sectors via write-blocked streaming validation.
					</p>
				</div>

				<div className="flex items-center space-x-2.5">
					<Button
						variant="outline"
						size="sm"
						onClick={loadDevices}
						className="text-xs flex items-center gap-1.5"
						title="Refresh Storage Devices"
					>
						<RefreshCw className="h-3.5 w-3.5" />
						<span>Rescan Drives</span>
					</Button>

					<Button
						size="sm"
						onClick={handleOpenReport}
						className="bg-emerald-600 hover:bg-emerald-700 text-white text-xs font-semibold flex items-center gap-1.5 shadow-sm"
					>
						<FileCheck2 className="h-3.5 w-3.5" />
						<span>Chain of Custody Certificate</span>
					</Button>
				</div>
			</div>

			{/* Active Progress Tracker (if a task is active or finished) */}
			{progress && (
				<CarverProgressTracker progress={progress} onStop={handleStopCarve} isStopping={isStopping} />
			)}

			{/* Main Workspace Tabs */}
			<Tabs value={activeTab} onValueChange={setActiveTab} className="w-full flex flex-col gap-4">
				<TabsList className="bg-muted/60 p-1 rounded-xl border border-border/60 self-start">
					<TabsTrigger value="scanner" className="text-xs font-semibold flex items-center gap-1.5">
						<SlidersHorizontal className="h-3.5 w-3.5" />
						<span>Carving Configuration</span>
					</TabsTrigger>
					<TabsTrigger value="locker" className="text-xs font-semibold flex items-center gap-1.5">
						<FolderArchive className="h-3.5 w-3.5" />
						<span>Evidence Locker ({artifacts.length})</span>
					</TabsTrigger>
				</TabsList>

				{/* Tab 1: Carving Configuration */}
				<TabsContent value="scanner" className="flex flex-col gap-6 m-0">
					<CarverConfigPanel
						devices={devices}
						selectedDevice={selectedDevice}
						onSelectDevice={setSelectedDevice}
						onStartCarve={handleStartCarve}
						isScanning={isScanning}
					/>

					{/* Mini Evidence Peek if artifacts carved */}
					{artifacts.length > 0 && (
						<div className="flex flex-col gap-3 pt-2">
							<div className="flex items-center justify-between">
								<h3 className="text-sm font-bold text-foreground flex items-center gap-2">
									<span>Recent Carved Evidence</span>
									<Badge variant="secondary" className="text-xs font-mono">
										{artifacts.length} Discovered
									</Badge>
								</h3>
								<Button
									variant="link"
									size="sm"
									onClick={() => setActiveTab("locker")}
									className="text-xs text-emerald-600 dark:text-emerald-400 p-0 h-auto"
								>
									View all in Evidence Locker &rarr;
								</Button>
							</div>
							<EvidenceGallery artifacts={artifacts.slice(0, 4)} isScanning={isScanning} />
						</div>
					)}
				</TabsContent>

				{/* Tab 2: Evidence Locker & Analysis */}
				<TabsContent value="locker" className="m-0">
					<EvidenceGallery artifacts={artifacts} isScanning={isScanning} />
				</TabsContent>
			</Tabs>

			{/* Chain of Custody Certificate Modal */}
			<ChainOfCustodyModal
				report={report}
				isOpen={isReportModalOpen}
				onClose={() => setIsReportModalOpen(false)}
			/>
		</div>
	);
}
