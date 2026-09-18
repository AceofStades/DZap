"use client";

import React, { useState, useEffect } from "react";
import {
	HardDrive,
	ShieldAlert,
	FolderOutput,
	Cpu,
	Layers,
	Play,
	Sliders,
	Image as ImageIcon,
	Video as VideoIcon,
	FileText,
	AlertCircle,
	CheckCircle2,
	Lock,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import type { StorageDevice, CarveTarget } from "@/lib/types";

interface CarverConfigPanelProps {
	devices: StorageDevice[];
	selectedDevice: StorageDevice | null;
	onSelectDevice: (device: StorageDevice) => void;
	onStartCarve: (target: CarveTarget) => void;
	isScanning: boolean;
}

export function CarverConfigPanel({
	devices,
	selectedDevice,
	onSelectDevice,
	onStartCarve,
	isScanning,
}: CarverConfigPanelProps) {
	const [outputDir, setOutputDir] = useState<string>("/tmp/dzap_carved");
	const [clusterSize, setClusterSize] = useState<number>(4096);
	const [scanMode, setScanMode] = useState<"fast_signature" | "deep_bifragment">("deep_bifragment");
	const [enabledTypes, setEnabledTypes] = useState<string[]>(["jpeg", "png", "mp4"]);
	const [validationError, setValidationError] = useState<string | null>(null);

	const toggleType = (type: string) => {
		if (enabledTypes.includes(type)) {
			if (enabledTypes.length === 1) return; // Keep at least one
			setEnabledTypes(enabledTypes.filter((t) => t !== type));
		} else {
			setEnabledTypes([...enabledTypes, type]);
		}
	};

	const handleLaunch = () => {
		if (!selectedDevice) {
			setValidationError("Please select a target evidence drive.");
			return;
		}
		if (!outputDir.trim()) {
			setValidationError("Please specify a valid extraction destination folder.");
			return;
		}
		if (outputDir.trim().startsWith(selectedDevice.name) || outputDir.trim().startsWith("/dev")) {
			setValidationError("Forensic Safety Violation: Output destination cannot reside on the source evidence drive!");
			return;
		}

		setValidationError(null);
		onStartCarve({
			device_path: selectedDevice.name,
			output_dir: outputDir.trim(),
			file_types: enabledTypes,
			cluster_size: clusterSize,
			scan_mode: scanMode,
		});
	};

	return (
		<div className="flex flex-col gap-5 w-full">
			{/* Target Evidence Selection */}
			<Card className="border-border/60 bg-card/60 backdrop-blur-sm shadow-sm">
				<CardHeader className="pb-3">
					<div className="flex items-center justify-between">
						<div className="flex items-center space-x-2.5">
							<HardDrive className="h-5 w-5 text-emerald-500" />
							<CardTitle className="text-base font-semibold">Evidence Source Drive</CardTitle>
						</div>
						<Badge
							variant="outline"
							className="bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 border-emerald-500/30 text-xs font-mono flex items-center gap-1"
						>
							<Lock className="h-3 w-3" />
							READ-ONLY FORENSIC ACCESS
						</Badge>
					</div>
					<CardDescription className="text-xs text-muted-foreground">
						Select the physical drive or partition to analyze. Read operations are strictly write-blocked to preserve evidence integrity.
					</CardDescription>
				</CardHeader>
				<CardContent>
					{devices.length === 0 ? (
						<div className="p-4 rounded-lg border border-dashed border-border text-center text-xs text-muted-foreground">
							No physical drives discovered. Connect an evidence disk or refresh devices.
						</div>
					) : (
						<div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-3">
							{devices.map((dev) => {
								const isSelected = selectedDevice?.name === dev.name;
								return (
									<button
										key={dev.name}
										type="button"
										onClick={() => onSelectDevice(dev)}
										disabled={isScanning}
										className={`text-left p-3.5 rounded-lg border transition-all flex flex-col justify-between ${
											isSelected
												? "border-emerald-500 bg-emerald-500/10 shadow-sm ring-1 ring-emerald-500"
												: "border-border/80 bg-background/50 hover:bg-accent/40 hover:border-border"
										} ${isScanning ? "opacity-60 cursor-not-allowed" : "cursor-pointer"}`}
									>
										<div className="flex items-center justify-between w-full mb-2">
											<div className="flex items-center space-x-2">
												<HardDrive className={`h-4 w-4 ${isSelected ? "text-emerald-500" : "text-muted-foreground"}`} />
												<span className="font-mono text-sm font-semibold">{dev.name}</span>
											</div>
											<Badge variant="secondary" className="text-xs font-medium">
												{dev.size}
											</Badge>
										</div>
										<div className="text-xs text-muted-foreground truncate w-full">
											{dev.model || "Generic Block Device"}
										</div>
										<div className="flex items-center justify-between mt-2 pt-2 border-t border-border/40 text-[11px] text-muted-foreground">
											<span>Transport: {dev.transport || "SATA/NVMe"}</span>
											{isSelected ? (
												<span className="text-emerald-600 dark:text-emerald-400 font-medium flex items-center gap-1">
													<CheckCircle2 className="h-3 w-3" /> Selected
												</span>
											) : (
												<span>Click to Select</span>
											)}
										</div>
									</button>
								);
							})}
						</div>
					)}
				</CardContent>
			</Card>

			{/* Carving Methodology & Target Types */}
			<div className="grid grid-cols-1 md:grid-cols-2 gap-5">
				{/* Carve Strategy */}
				<Card className="border-border/60 bg-card/60 backdrop-blur-sm shadow-sm">
					<CardHeader className="pb-3">
						<div className="flex items-center space-x-2">
							<Layers className="h-4 w-4 text-emerald-500" />
							<CardTitle className="text-sm font-semibold">Forensic Reconstruction Engine</CardTitle>
						</div>
						<CardDescription className="text-xs text-muted-foreground">
							Choose standard contiguous carving or advanced bi-fragment gap reassembly.
						</CardDescription>
					</CardHeader>
					<CardContent className="flex flex-col gap-3">
						<button
							type="button"
							onClick={() => setScanMode("deep_bifragment")}
							disabled={isScanning}
							className={`p-3 rounded-lg border text-left transition-all ${
								scanMode === "deep_bifragment"
									? "border-emerald-500 bg-emerald-500/10 ring-1 ring-emerald-500"
									: "border-border/70 hover:bg-accent/40"
							}`}
						>
							<div className="flex items-center justify-between mb-1">
								<span className="text-xs font-bold text-foreground">Deep Bifragment Reassembly</span>
								<Badge className="bg-emerald-600 text-white text-[10px] h-4">Garfinkel (2007)</Badge>
							</div>
							<p className="text-[11px] text-muted-foreground">
								Performs internal structural parsing and cluster-aligned fragment gap analysis to stitch discontinuous files together.
							</p>
						</button>

						<button
							type="button"
							onClick={() => setScanMode("fast_signature")}
							disabled={isScanning}
							className={`p-3 rounded-lg border text-left transition-all ${
								scanMode === "fast_signature"
									? "border-emerald-500 bg-emerald-500/10 ring-1 ring-emerald-500"
									: "border-border/70 hover:bg-accent/40"
							}`}
						>
							<div className="flex items-center justify-between mb-1">
								<span className="text-xs font-bold text-foreground">Linear Contiguous Carving</span>
								<Badge variant="outline" className="text-[10px] h-4">Scalpel (2005)</Badge>
							</div>
							<p className="text-[11px] text-muted-foreground">
								High-speed sliding window matching pre-compiled signature tables on contiguous cluster boundaries.
							</p>
						</button>

						{/* Cluster Alignment */}
						<div className="pt-2 flex items-center justify-between">
							<Label htmlFor="cluster-size" className="text-xs font-medium text-foreground">
								Sector / Cluster Alignment:
							</Label>
							<select
								id="cluster-size"
								value={clusterSize}
								onChange={(e) => setClusterSize(Number(e.target.value))}
								disabled={isScanning}
								className="bg-background border border-border rounded px-2.5 py-1 text-xs text-foreground focus:outline-none focus:ring-1 focus:ring-emerald-500"
							>
								<option value={512}>512 Bytes (Standard Sector)</option>
								<option value={1024}>1024 Bytes (1 KB)</option>
								<option value={2048}>2048 Bytes (2 KB)</option>
								<option value={4096}>4096 Bytes (4 KB Default)</option>
								<option value={8192}>8192 Bytes (8 KB)</option>
							</select>
						</div>
					</CardContent>
				</Card>

				{/* Target File Signatures & Destination */}
				<Card className="border-border/60 bg-card/60 backdrop-blur-sm shadow-sm">
					<CardHeader className="pb-3">
						<div className="flex items-center space-x-2">
							<Sliders className="h-4 w-4 text-emerald-500" />
							<CardTitle className="text-sm font-semibold">Target Signatures & Extraction</CardTitle>
						</div>
						<CardDescription className="text-xs text-muted-foreground">
							Select file types to recover and destination on your host filesystem.
						</CardDescription>
					</CardHeader>
					<CardContent className="flex flex-col gap-4">
						<div>
							<Label className="text-xs font-medium mb-2 block">Enabled File Types:</Label>
							<div className="flex flex-wrap gap-2">
								<button
									type="button"
									onClick={() => toggleType("jpeg")}
									disabled={isScanning}
									className={`px-3 py-1.5 rounded-md text-xs font-medium border flex items-center gap-1.5 transition-all ${
										enabledTypes.includes("jpeg")
											? "border-emerald-500 bg-emerald-500/15 text-emerald-700 dark:text-emerald-300"
											: "border-border text-muted-foreground opacity-60"
									}`}
								>
									<ImageIcon className="h-3.5 w-3.5" />
									JPEG / JFIF
								</button>
								<button
									type="button"
									onClick={() => toggleType("png")}
									disabled={isScanning}
									className={`px-3 py-1.5 rounded-md text-xs font-medium border flex items-center gap-1.5 transition-all ${
										enabledTypes.includes("png")
											? "border-emerald-500 bg-emerald-500/15 text-emerald-700 dark:text-emerald-300"
											: "border-border text-muted-foreground opacity-60"
									}`}
								>
									<ImageIcon className="h-3.5 w-3.5" />
									PNG (CRC32)
								</button>
								<button
									type="button"
									onClick={() => toggleType("mp4")}
									disabled={isScanning}
									className={`px-3 py-1.5 rounded-md text-xs font-medium border flex items-center gap-1.5 transition-all ${
										enabledTypes.includes("mp4")
											? "border-emerald-500 bg-emerald-500/15 text-emerald-700 dark:text-emerald-300"
											: "border-border text-muted-foreground opacity-60"
									}`}
								>
									<VideoIcon className="h-3.5 w-3.5" />
									MP4 / ISO Video
								</button>
								<button
									type="button"
									disabled
									className="px-3 py-1.5 rounded-md text-xs font-medium border border-border/40 text-muted-foreground/50 flex items-center gap-1.5 cursor-not-allowed"
									title="PDF & DOCX signatures planned for subsequent release"
								>
									<FileText className="h-3.5 w-3.5" />
									PDF / Office (Planned)
								</button>
							</div>
						</div>

						{/* Output Directory */}
						<div>
							<Label htmlFor="output-dir" className="text-xs font-medium mb-1.5 block flex items-center gap-1.5">
								<FolderOutput className="h-3.5 w-3.5 text-emerald-500" />
								Safe Extraction Directory:
							</Label>
							<Input
								id="output-dir"
								value={outputDir}
								onChange={(e) => setOutputDir(e.target.value)}
								disabled={isScanning}
								placeholder="/tmp/dzap_carved"
								className="text-xs font-mono bg-background"
							/>
							<p className="text-[11px] text-muted-foreground mt-1">
								Must reside on a separate mounted disk to ensure evidence source remains pristine.
							</p>
						</div>
					</CardContent>
				</Card>
			</div>

			{/* Validation Warning */}
			{validationError && (
				<div className="p-3 rounded-lg bg-rose-500/10 border border-rose-500/30 text-rose-600 dark:text-rose-400 text-xs flex items-center gap-2">
					<AlertCircle className="h-4 w-4 shrink-0" />
					<span>{validationError}</span>
				</div>
			)}

			{/* Action Launch Bar */}
			<div className="flex items-center justify-between p-4 rounded-xl border border-border/80 bg-card/80 backdrop-blur shadow-sm">
				<div className="flex items-center space-x-3 text-xs text-muted-foreground">
					<Cpu className="h-4 w-4 text-emerald-500 shrink-0" />
					<span>
						Target: <strong className="text-foreground">{selectedDevice?.name || "None"}</strong> | Mode:{" "}
						<strong className="text-foreground">{scanMode === "deep_bifragment" ? "Deep Bifragment" : "Contiguous Scalpel"}</strong> |
						Cluster: <strong className="text-foreground">{clusterSize} B</strong>
					</span>
				</div>
				<Button
					onClick={handleLaunch}
					disabled={isScanning || !selectedDevice}
					className="bg-emerald-600 hover:bg-emerald-700 text-white font-semibold text-xs px-5 shadow-sm flex items-center space-x-2"
				>
					<Play className="h-3.5 w-3.5 fill-current" />
					<span>Start Forensic Carving</span>
				</Button>
			</div>
		</div>
	);
}
