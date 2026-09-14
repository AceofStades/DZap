"use client";

import { useEffect, useMemo, useState } from "react";
import { Files, KeyRound, Loader2, ScanSearch, Search } from "lucide-react";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
} from "@/components/ui/select";
import type {
	RecoveryJobRecord,
	RecoveryMethod,
	RecoveryVolume,
} from "@/lib/types";
import {
	analyzeRecoveryImage,
	getRecoveryVolumes,
	startRecoveryExtraction,
} from "@/lib/utils";

function formatBytes(value: string) {
	const bytes = Number(value);
	if (!Number.isFinite(bytes) || bytes < 0) return "Unknown size";
	const units = ["B", "KiB", "MiB", "GiB", "TiB"];
	const index =
		bytes === 0
			? 0
			: Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
	return `${(bytes / 1024 ** index).toFixed(index === 0 ? 0 : 2)} ${units[index]}`;
}

function volumeLabel(volume: RecoveryVolume) {
	const format = volume.encryption
		? `${volume.encryption.toUpperCase()} encrypted`
		: volume.filesystem?.toUpperCase() || "unknown filesystem";
	return `${volume.id} · ${format} · ${formatBytes(volume.sizeBytes)}${volume.label ? ` · ${volume.label}` : ""}`;
}

export function RecoveryMethodPicker({
	job,
	onChanged,
}: {
	job: RecoveryJobRecord;
	onChanged: () => Promise<void>;
}) {
	const [volumes, setVolumes] = useState<RecoveryVolume[]>([]);
	const [selectedId, setSelectedId] = useState<string | null>(null);
	const [method, setMethod] = useState<RecoveryMethod>("filesystem_copy");
	const [passphrase, setPassphrase] = useState("");
	const [loading, setLoading] = useState(true);
	const [running, setRunning] = useState(false);
	const [analyzing, setAnalyzing] = useState(false);
	const [error, setError] = useState<string | null>(null);

	useEffect(() => {
		let disposed = false;
		setLoading(true);
		setError(null);
		void getRecoveryVolumes(job.id)
			.then((records) => {
				if (disposed) return;
				setVolumes(records);
				const preferred =
					records.find((volume) => volume.filesystemCopySupported) ??
					records.find((volume) => volume.photorecSupported) ??
					null;
				setSelectedId(preferred?.id ?? null);
				setMethod(
					preferred?.filesystemCopySupported
						? "filesystem_copy"
						: "photorec",
				);
			})
			.catch((loadError) => {
				if (disposed) return;
				setError(
					loadError instanceof Error
						? loadError.message
						: "Failed to inspect recovery volumes.",
				);
			})
			.finally(() => {
				if (!disposed) setLoading(false);
			});
		return () => {
			disposed = true;
		};
	}, [job.id]);

	const selected = useMemo(
		() => volumes.find((volume) => volume.id === selectedId) ?? null,
		[volumes, selectedId],
	);

	const runAnalysis = async () => {
		setAnalyzing(true);
		setError(null);
		try {
			await analyzeRecoveryImage(job.id);
			await onChanged();
		} catch (analysisError) {
			setError(
				analysisError instanceof Error
					? analysisError.message
					: "Failed to run TestDisk analysis.",
			);
		} finally {
			setAnalyzing(false);
		}
	};

	const startRecovery = async () => {
		if (!selected) return;
		setRunning(true);
		setError(null);
		try {
			await startRecoveryExtraction(
				job.id,
				method,
				selected.id,
				selected.encryption ? passphrase : undefined,
			);
			setPassphrase("");
			await onChanged();
		} catch (startError) {
			setPassphrase("");
			setError(
				startError instanceof Error
					? startError.message
					: "Failed to start file recovery.",
			);
		} finally {
			setRunning(false);
		}
	};

	return (
		<div className="space-y-4 rounded-md border p-4">
			<div>
				<h3 className="font-semibold">Recover files from the image</h3>
				<p className="mt-1 text-sm text-muted-foreground">
					Try read-only filesystem copy first. Use PhotoRec when filesystem
					metadata is too damaged to mount.
				</p>
			</div>

			{job.status === "image_complete" && (
				<Button
					variant="outline"
					onClick={() => void runAnalysis()}
					disabled={analyzing || running}
				>
					{analyzing ? (
						<Loader2 className="mr-2 h-4 w-4 animate-spin" />
					) : (
						<Search className="mr-2 h-4 w-4" />
					)}
					Run read-only TestDisk analysis
				</Button>
			)}

			{job.testdiskAnalysis && (
				<Alert>
					<ScanSearch className="h-4 w-4" />
					<AlertDescription className="space-y-1">
						<p>
							TestDisk {job.testdiskAnalysis.successful ? "completed" : "reported a failure"}.
						</p>
						{job.testdiskAnalysis.summary && (
							<pre className="max-h-36 overflow-auto whitespace-pre-wrap text-xs">
								{job.testdiskAnalysis.summary}
							</pre>
						)}
					</AlertDescription>
				</Alert>
			)}

			{error && (
				<Alert variant="destructive">
					<AlertDescription>{error}</AlertDescription>
				</Alert>
			)}

			{loading ? (
				<p className="flex items-center text-sm text-muted-foreground">
					<Loader2 className="mr-2 h-4 w-4 animate-spin" />
					Inspecting image volumes read-only…
				</p>
			) : volumes.length === 0 ? (
				<p className="text-sm text-muted-foreground">
					No image volumes were detected. TestDisk diagnostics and whole-image
					carving may still help.
				</p>
			) : (
				<>
					<Select
						value={selectedId ?? undefined}
						onValueChange={(value) => {
							const volume = volumes.find((candidate) => candidate.id === value);
							setSelectedId(value);
							setPassphrase("");
							if (volume && !volume.filesystemCopySupported) {
								setMethod("photorec");
							}
						}}
					>
						<SelectTrigger>
							<SelectValue placeholder="Select an image volume" />
						</SelectTrigger>
						<SelectContent>
							{volumes.map((volume) => (
								<SelectItem key={volume.id} value={volume.id}>
									{volumeLabel(volume)}
								</SelectItem>
							))}
						</SelectContent>
					</Select>

					<div className="grid gap-3 sm:grid-cols-2">
						<button
							type="button"
							onClick={() => setMethod("filesystem_copy")}
							disabled={!selected?.filesystemCopySupported}
							className={`rounded-md border p-3 text-left ${
								method === "filesystem_copy" ? "border-primary ring-1 ring-primary" : ""
							} disabled:cursor-not-allowed disabled:opacity-50`}
						>
							<p className="flex items-center font-medium">
								<Files className="mr-2 h-4 w-4" />
								Filesystem copy
							</p>
							<p className="mt-1 text-xs text-muted-foreground">
								Preserves readable names and folders and hashes every copied file.
							</p>
						</button>
						<button
							type="button"
							onClick={() => setMethod("photorec")}
							disabled={!selected?.photorecSupported}
							className={`rounded-md border p-3 text-left ${
								method === "photorec" ? "border-primary ring-1 ring-primary" : ""
							} disabled:cursor-not-allowed disabled:opacity-50`}
						>
							<p className="flex items-center font-medium">
								<ScanSearch className="mr-2 h-4 w-4" />
								PhotoRec carving
							</p>
							<p className="mt-1 text-xs text-muted-foreground">
								Scans file signatures; original names and folder structure are usually lost.
							</p>
						</button>
					</div>

					{selected?.encryption && (
						<div className="space-y-2">
							<label htmlFor="recovery-passphrase" className="flex items-center text-sm font-medium">
								<KeyRound className="mr-2 h-4 w-4" />
								{selected.encryption.toUpperCase()} unlock secret
							</label>
							<Input
								id="recovery-passphrase"
								type="password"
								autoComplete="off"
								value={passphrase}
								onChange={(event) => setPassphrase(event.target.value)}
								placeholder="Used once and never persisted"
							/>
						</div>
					)}

					<Button
						onClick={() => void startRecovery()}
						disabled={
							running ||
							!selected ||
							(selected.encryption !== null && passphrase.length === 0)
						}
					>
						{running && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
						Start {method === "filesystem_copy" ? "filesystem copy" : "PhotoRec carving"}
					</Button>
				</>
			)}
		</div>
	);
}
