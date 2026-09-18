"use client";

import React, { useState, useMemo } from "react";
import {
	Image as ImageIcon,
	Video as VideoIcon,
	FileText,
	CheckCircle2,
	AlertCircle,
	Copy,
	Check,
	ExternalLink,
	Eye,
	Filter,
	Search,
	Download,
	Layers,
	Binary,
	X,
	FileCode,
	Maximize2,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import type { CarvedArtifact } from "@/lib/types";
import { getCarvePreviewUrl } from "@/lib/utils";

interface EvidenceGalleryProps {
	artifacts: CarvedArtifact[];
	isScanning: boolean;
}

export function EvidenceGallery({ artifacts, isScanning }: EvidenceGalleryProps) {
	const [activeFilter, setActiveFilter] = useState<string>("all");
	const [searchQuery, setSearchQuery] = useState<string>("");
	const [selectedArtifact, setSelectedArtifact] = useState<CarvedArtifact | null>(null);
	const [copiedHash, setCopiedHash] = useState<string | null>(null);
	const [viewMode, setViewMode] = useState<"grid" | "table">("grid");

	const handleCopyHash = (hash: string, e?: React.MouseEvent) => {
		if (e) e.stopPropagation();
		navigator.clipboard.writeText(hash);
		setCopiedHash(hash);
		setTimeout(() => setCopiedHash(null), 2000);
	};

	const filteredArtifacts = useMemo(() => {
		return artifacts.filter((a) => {
			// Type filter
			if (activeFilter === "images" && !["jpg", "jpeg", "png"].includes(a.extension.toLowerCase())) return false;
			if (activeFilter === "videos" && !["mp4", "mov", "m4v"].includes(a.extension.toLowerCase())) return false;
			if (activeFilter === "high" && a.confidence_score < 0.9) return false;
			if (activeFilter === "medium" && (a.confidence_score < 0.5 || a.confidence_score >= 0.9)) return false;
			if (activeFilter === "fragmented" && !a.is_fragmented) return false;

			// Text search
			if (searchQuery.trim()) {
				const query = searchQuery.toLowerCase();
				const matchesName = a.filename.toLowerCase().includes(query);
				const matchesHash = a.sha256.toLowerCase().includes(query);
				const matchesSector = a.start_sector.toString().includes(query);
				if (!matchesName && !matchesHash && !matchesSector) return false;
			}

			return true;
		});
	}, [artifacts, activeFilter, searchQuery]);

	const formatBytes = (bytes: number): string => {
		if (bytes === 0) return "0 B";
		const k = 1024;
		const sizes = ["B", "KB", "MB", "GB"];
		const i = Math.floor(Math.log(bytes) / Math.log(k));
		return `${parseFloat((bytes / Math.pow(k, i)).toFixed(1))} ${sizes[i]}`;
	};

	const getConfidenceColor = (score: number) => {
		if (score >= 0.9) {
			return "bg-emerald-500/15 text-emerald-600 dark:text-emerald-400 border-emerald-500/30";
		}
		if (score >= 0.5) {
			return "bg-amber-500/15 text-amber-600 dark:text-amber-400 border-amber-500/30";
		}
		return "bg-rose-500/15 text-rose-600 dark:text-rose-400 border-rose-500/30";
	};

	return (
		<div className="flex flex-col gap-4 w-full">
			{/* Toolbar: Search, Filters & View Mode */}
			<div className="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-3 p-3 rounded-xl border border-border/70 bg-card/60 backdrop-blur-sm shadow-sm">
				{/* Filters */}
				<div className="flex flex-wrap items-center gap-1.5">
					<button
						type="button"
						onClick={() => setActiveFilter("all")}
						className={`px-3 py-1 rounded-md text-xs font-medium transition-all ${
							activeFilter === "all"
								? "bg-emerald-600 text-white shadow-sm"
								: "text-muted-foreground hover:bg-accent hover:text-foreground"
						}`}
					>
						All ({artifacts.length})
					</button>
					<button
						type="button"
						onClick={() => setActiveFilter("images")}
						className={`px-3 py-1 rounded-md text-xs font-medium transition-all ${
							activeFilter === "images"
								? "bg-emerald-600 text-white shadow-sm"
								: "text-muted-foreground hover:bg-accent hover:text-foreground"
						}`}
					>
						Images
					</button>
					<button
						type="button"
						onClick={() => setActiveFilter("videos")}
						className={`px-3 py-1 rounded-md text-xs font-medium transition-all ${
							activeFilter === "videos"
								? "bg-emerald-600 text-white shadow-sm"
								: "text-muted-foreground hover:bg-accent hover:text-foreground"
						}`}
					>
						Videos
					</button>
					<button
						type="button"
						onClick={() => setActiveFilter("high")}
						className={`px-3 py-1 rounded-md text-xs font-medium transition-all ${
							activeFilter === "high"
								? "bg-emerald-600 text-white shadow-sm"
								: "text-muted-foreground hover:bg-accent hover:text-foreground"
						}`}
					>
						High Confidence (&ge;90%)
					</button>
					<button
						type="button"
						onClick={() => setActiveFilter("fragmented")}
						className={`px-3 py-1 rounded-md text-xs font-medium transition-all ${
							activeFilter === "fragmented"
								? "bg-emerald-600 text-white shadow-sm"
								: "text-muted-foreground hover:bg-accent hover:text-foreground"
						}`}
					>
						Reassembled Gaps
					</button>
				</div>

				{/* Search & Layout toggle */}
				<div className="flex items-center space-x-2 w-full sm:w-auto">
					<div className="relative flex-1 sm:w-48">
						<Search className="h-3.5 w-3.5 absolute left-2.5 top-2.5 text-muted-foreground" />
						<Input
							value={searchQuery}
							onChange={(e) => setSearchQuery(e.target.value)}
							placeholder="Search by hash/sector..."
							className="pl-8 h-8 text-xs bg-background"
						/>
					</div>
					<div className="flex border border-border rounded-md overflow-hidden bg-background">
						<button
							type="button"
							onClick={() => setViewMode("grid")}
							className={`px-2.5 py-1 text-xs font-medium ${
								viewMode === "grid" ? "bg-accent text-accent-foreground" : "text-muted-foreground hover:text-foreground"
							}`}
							title="Grid View"
						>
							Grid
						</button>
						<button
							type="button"
							onClick={() => setViewMode("table")}
							className={`px-2.5 py-1 text-xs font-medium ${
								viewMode === "table" ? "bg-accent text-accent-foreground" : "text-muted-foreground hover:text-foreground"
							}`}
							title="Table View"
						>
							Table
						</button>
					</div>
				</div>
			</div>

			{/* Empty State */}
			{artifacts.length === 0 ? (
				<Card className="border-border/60 bg-card/40 backdrop-blur-sm p-10 text-center">
					<div className="flex flex-col items-center justify-center max-w-sm mx-auto text-center">
						<div className="p-3.5 rounded-full bg-emerald-500/10 text-emerald-600 dark:text-emerald-400 mb-3">
							<Binary className="h-6 w-6" />
						</div>
						<h4 className="text-sm font-semibold text-foreground mb-1">
							{isScanning ? "Scanning Storage Sectors..." : "No Carved Evidence Yet"}
						</h4>
						<p className="text-xs text-muted-foreground">
							{isScanning
								? "The carving engine is probing cluster boundaries for file headers, signatures, and fragments. Carved artifacts will appear here in real time."
								: "Select an evidence drive above and launch the forensic carver to discover deleted, wiped, or unallocated files."}
						</p>
					</div>
				</Card>
			) : filteredArtifacts.length === 0 ? (
				<div className="p-8 text-center text-xs text-muted-foreground border border-dashed border-border rounded-xl">
					No artifacts matched the active filter or search query.
				</div>
			) : viewMode === "grid" ? (
				/* Grid View */
				<div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4">
					{filteredArtifacts.map((artifact) => {
						const isImage = ["jpg", "jpeg", "png"].includes(artifact.extension.toLowerCase());
						const isVideo = ["mp4", "mov", "m4v"].includes(artifact.extension.toLowerCase());

						return (
							<div
								key={artifact.id}
								onClick={() => setSelectedArtifact(artifact)}
								className="group relative rounded-xl border border-border/70 bg-card/70 hover:border-emerald-500/80 hover:shadow-md transition-all cursor-pointer overflow-hidden flex flex-col justify-between"
							>
								{/* Card Visual Header / Thumbnail */}
								<div className="h-36 w-full bg-muted/40 relative flex items-center justify-center overflow-hidden border-b border-border/40">
									{artifact.preview_data_url ? (
										<img
											src={artifact.preview_data_url}
											alt={artifact.filename}
											className="w-full h-full object-cover group-hover:scale-105 transition-transform duration-300"
										/>
									) : isImage ? (
										<img
											src={getCarvePreviewUrl(artifact.id)}
											alt={artifact.filename}
											onError={(e) => {
												// Fallback if not directly readable
												e.currentTarget.style.display = "none";
											}}
											className="w-full h-full object-cover group-hover:scale-105 transition-transform duration-300"
										/>
									) : (
										<div className="flex flex-col items-center gap-2 text-muted-foreground">
											{isVideo ? <VideoIcon className="h-8 w-8 text-blue-500/80" /> : <FileText className="h-8 w-8" />}
											<span className="text-[11px] font-mono uppercase tracking-wider">{artifact.extension}</span>
										</div>
									)}

									{/* Badges Overlay */}
									<div className="absolute top-2 left-2 flex gap-1.5">
										<Badge variant="outline" className={`text-[10px] font-mono backdrop-blur-md ${getConfidenceColor(artifact.confidence_score)}`}>
											{Math.round(artifact.confidence_score * 100)}% Conf
										</Badge>
										{artifact.is_fragmented && (
											<Badge className="bg-amber-600/90 text-white text-[10px] backdrop-blur-md flex items-center gap-1">
												<Layers className="h-2.5 w-2.5" /> Reassembled
											</Badge>
										)}
									</div>

									{/* Hover Quick Action */}
									<div className="absolute inset-0 bg-black/40 opacity-0 group-hover:opacity-100 transition-opacity flex items-center justify-center">
										<span className="px-3 py-1.5 rounded-md bg-background/90 text-foreground text-xs font-semibold shadow flex items-center gap-1.5">
											<Eye className="h-3.5 w-3.5" /> Inspect Evidence
										</span>
									</div>
								</div>

								{/* Card Details */}
								<div className="p-3.5 flex flex-col gap-2">
									<div className="flex items-center justify-between">
										<span className="font-mono text-xs font-bold text-foreground truncate max-w-[170px]" title={artifact.filename}>
											{artifact.filename}
										</span>
										<span className="text-[11px] font-mono text-muted-foreground">{formatBytes(artifact.size_bytes)}</span>
									</div>

									<div className="flex items-center justify-between text-[11px] text-muted-foreground font-mono">
										<span>Sectors: #{artifact.start_sector}..{artifact.end_sector}</span>
										<span>{artifact.file_type.split(" ")[0]}</span>
									</div>

									{/* SHA-256 Digest Pill */}
									<div className="flex items-center justify-between pt-2 border-t border-border/40 text-[10px] font-mono">
										<span className="text-muted-foreground truncate max-w-[170px]">
											SHA256: {artifact.sha256.slice(0, 14)}...
										</span>
										<button
											type="button"
											onClick={(e) => handleCopyHash(artifact.sha256, e)}
											className="p-1 rounded hover:bg-accent text-muted-foreground hover:text-foreground transition-colors"
											title="Copy SHA-256 Hash"
										>
											{copiedHash === artifact.sha256 ? (
												<Check className="h-3 w-3 text-emerald-500" />
											) : (
												<Copy className="h-3 w-3" />
											)}
										</button>
									</div>
								</div>
							</div>
						);
					})}
				</div>
			) : (
				/* Table View */
				<div className="rounded-xl border border-border/70 overflow-x-auto bg-card/60 backdrop-blur-sm shadow-sm">
					<table className="w-full text-xs text-left font-mono">
						<thead className="bg-muted/40 border-b border-border/60 text-muted-foreground">
							<tr>
								<th className="py-2.5 px-3">Artifact ID</th>
								<th className="py-2.5 px-3">Filename</th>
								<th className="py-2.5 px-3">Type</th>
								<th className="py-2.5 px-3">Size</th>
								<th className="py-2.5 px-3">Start Sector</th>
								<th className="py-2.5 px-3">Confidence</th>
								<th className="py-2.5 px-3">SHA-256 Digest</th>
								<th className="py-2.5 px-3 text-right">Action</th>
							</tr>
						</thead>
						<tbody className="divide-y divide-border/40">
							{filteredArtifacts.map((a) => (
								<tr
									key={a.id}
									onClick={() => setSelectedArtifact(a)}
									className="hover:bg-accent/40 cursor-pointer transition-colors"
								>
									<td className="py-2.5 px-3 text-emerald-600 dark:text-emerald-400 font-bold">{a.id}</td>
									<td className="py-2.5 px-3 font-semibold text-foreground truncate max-w-[180px]">{a.filename}</td>
									<td className="py-2.5 px-3 text-muted-foreground">{a.file_type}</td>
									<td className="py-2.5 px-3">{formatBytes(a.size_bytes)}</td>
									<td className="py-2.5 px-3">#{a.start_sector.toLocaleString()}</td>
									<td className="py-2.5 px-3">
										<Badge variant="outline" className={`text-[10px] ${getConfidenceColor(a.confidence_score)}`}>
											{Math.round(a.confidence_score * 100)}%
										</Badge>
									</td>
									<td className="py-2.5 px-3 text-muted-foreground truncate max-w-[140px]" title={a.sha256}>
										{a.sha256.slice(0, 16)}...
									</td>
									<td className="py-2.5 px-3 text-right">
										<Button variant="ghost" size="sm" className="h-7 px-2 text-xs">
											Inspect
										</Button>
									</td>
								</tr>
							))}
						</tbody>
					</table>
				</div>
			)}

			{/* Forensic Inspection Modal */}
			{selectedArtifact && (
				<div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/70 backdrop-blur-sm animate-in fade-in duration-200">
					<div className="bg-card border border-border rounded-2xl shadow-2xl w-full max-w-3xl max-h-[90vh] overflow-hidden flex flex-col animate-in zoom-in-95 duration-200">
						{/* Modal Header */}
						<div className="px-6 py-4 border-b border-border/70 flex items-center justify-between bg-muted/20">
							<div className="flex items-center space-x-3">
								<div className="p-2 rounded-lg bg-emerald-500/10 text-emerald-600 dark:text-emerald-400">
									<Binary className="h-5 w-5" />
								</div>
								<div>
									<h3 className="text-base font-bold text-foreground flex items-center gap-2">
										<span>{selectedArtifact.filename}</span>
										<Badge variant="outline" className={`text-xs ${getConfidenceColor(selectedArtifact.confidence_score)}`}>
											{Math.round(selectedArtifact.confidence_score * 100)}% Confidence
										</Badge>
									</h3>
									<p className="text-xs text-muted-foreground font-mono">
										ID: {selectedArtifact.id} | Type: {selectedArtifact.file_type}
									</p>
								</div>
							</div>
							<button
								type="button"
								onClick={() => setSelectedArtifact(null)}
								className="p-1.5 rounded-lg hover:bg-accent text-muted-foreground hover:text-foreground transition-colors"
							>
								<X className="h-5 w-5" />
							</button>
						</div>

						{/* Modal Body */}
						<div className="p-6 overflow-y-auto flex flex-col gap-5">
							{/* Visual Preview / Media Player */}
							<div className="rounded-xl border border-border/60 bg-muted/20 overflow-hidden flex items-center justify-center min-h-[220px] max-h-[320px]">
								{selectedArtifact.preview_data_url ? (
									<img
										src={selectedArtifact.preview_data_url}
										alt={selectedArtifact.filename}
										className="max-h-[320px] w-auto object-contain"
									/>
								) : ["jpg", "jpeg", "png"].includes(selectedArtifact.extension.toLowerCase()) ? (
									<img
										src={getCarvePreviewUrl(selectedArtifact.id)}
										alt={selectedArtifact.filename}
										className="max-h-[320px] w-auto object-contain"
									/>
								) : (
									<div className="p-8 text-center text-muted-foreground flex flex-col items-center gap-2">
										<VideoIcon className="h-10 w-10 text-blue-500" />
										<span className="text-xs font-mono">Video Stream Carved ({formatBytes(selectedArtifact.size_bytes)})</span>
										<span className="text-[11px] text-muted-foreground">Ready for analysis or external playback</span>
									</div>
								)}
							</div>

							{/* Forensic Properties Grid */}
							<div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
								<div className="p-3 rounded-lg border border-border/60 bg-background/50">
									<span className="text-[11px] text-muted-foreground block">Carved Size</span>
									<span className="font-mono text-sm font-bold text-foreground">
										{formatBytes(selectedArtifact.size_bytes)}
									</span>
								</div>
								<div className="p-3 rounded-lg border border-border/60 bg-background/50">
									<span className="text-[11px] text-muted-foreground block">LBA Sector Range</span>
									<span className="font-mono text-sm font-bold text-foreground">
										#{selectedArtifact.start_sector}..{selectedArtifact.end_sector}
									</span>
								</div>
								<div className="p-3 rounded-lg border border-border/60 bg-background/50">
									<span className="text-[11px] text-muted-foreground block">Reassembly</span>
									<span className="font-mono text-sm font-bold text-foreground">
										{selectedArtifact.is_fragmented ? "Bifragment Gaps" : "Contiguous"}
									</span>
								</div>
								<div className="p-3 rounded-lg border border-border/60 bg-background/50">
									<span className="text-[11px] text-muted-foreground block">Byte Offset</span>
									<span className="font-mono text-sm font-bold text-foreground">
										0x{selectedArtifact.byte_offset.toString(16).toUpperCase()}
									</span>
								</div>
							</div>

							{/* Metadata Key-Value */}
							{Object.keys(selectedArtifact.metadata).length > 0 && (
								<div className="p-3.5 rounded-xl border border-border/60 bg-background/40">
									<span className="text-xs font-bold text-foreground mb-2 block flex items-center gap-1.5">
										<FileCode className="h-3.5 w-3.5 text-emerald-500" />
										Structural Metadata Parsed:
									</span>
									<div className="grid grid-cols-1 sm:grid-cols-2 gap-2 text-xs font-mono">
										{Object.entries(selectedArtifact.metadata).map(([k, v]) => (
											<div key={k} className="flex justify-between border-b border-border/30 pb-1">
												<span className="text-muted-foreground capitalize">{k}:</span>
												<span className="text-foreground font-semibold truncate max-w-[200px]">{v}</span>
											</div>
										))}
									</div>
								</div>
							)}

							{/* Cryptographic SHA-256 Hash */}
							<div className="p-3.5 rounded-xl border border-border/60 bg-background/40 flex flex-col gap-1.5">
								<div className="flex items-center justify-between text-xs">
									<span className="font-bold text-foreground flex items-center gap-1.5">
										<CheckCircle2 className="h-3.5 w-3.5 text-emerald-500" />
										Cryptographic Integrity Hash (SHA-256):
									</span>
									<Button
										variant="ghost"
										size="sm"
										onClick={() => handleCopyHash(selectedArtifact.sha256)}
										className="h-6 text-xs text-emerald-600 dark:text-emerald-400"
									>
										{copiedHash === selectedArtifact.sha256 ? (
											<>
												<Check className="h-3 w-3 mr-1" /> Copied!
											</>
										) : (
											<>
												<Copy className="h-3 w-3 mr-1" /> Copy Hash
											</>
										)}
									</Button>
								</div>
								<div className="p-2.5 rounded-lg bg-muted/40 font-mono text-[11px] text-foreground break-all select-all">
									{selectedArtifact.sha256}
								</div>
							</div>
						</div>

						{/* Modal Footer */}
						<div className="px-6 py-4 border-t border-border/70 flex items-center justify-between bg-muted/20">
							<span className="text-[11px] text-muted-foreground font-mono">
								Extracted to: {selectedArtifact.extracted_path}
							</span>
							<div className="flex items-center space-x-2">
								<Button variant="outline" size="sm" onClick={() => setSelectedArtifact(null)} className="text-xs">
									Close
								</Button>
								<Button
									size="sm"
									className="bg-emerald-600 hover:bg-emerald-700 text-white text-xs font-semibold flex items-center gap-1.5"
									onClick={() => {
										window.open(getCarvePreviewUrl(selectedArtifact.id), "_blank");
									}}
								>
									<Download className="h-3.5 w-3.5" />
									Download Artifact
								</Button>
							</div>
						</div>
					</div>
				</div>
			)}
		</div>
	);
}
