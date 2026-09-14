"use client";

import { useCallback, useEffect, useState } from "react";
import { useRouter } from "next/navigation";
import {
	CheckCircle2,
	HardDrive,
	Loader2,
	RefreshCw,
	Save,
	XCircle,
} from "lucide-react";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
} from "@/components/ui/select";
import type {
	RecoveryAssessment,
	RecoveryDestination,
	RecoveryImagePlan,
} from "@/lib/types";
import {
	getRecoveryDestinations,
	mountRecoveryDestination,
	planRecoveryImage,
	startRecoveryImage,
} from "@/lib/utils";

function formatBytes(value: string | null) {
	if (!value) return "Unknown";
	const bytes = Number(value);
	if (!Number.isFinite(bytes) || bytes < 0) return "Unknown";
	const units = ["B", "KiB", "MiB", "GiB", "TiB"];
	const index = bytes === 0
		? 0
		: Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
	return `${(bytes / 1024 ** index).toFixed(index === 0 ? 0 : 2)} ${units[index]}`;
}

function destinationKey(destination: RecoveryDestination) {
	return `${destination.driveIdentity.majorMinor}:${destination.deviceMajorMinor}`;
}

function destinationLabel(destination: RecoveryDestination) {
	const name = destination.driveIdentity.model || destination.drivePath;
	const available = destination.availableBytes
		? `${formatBytes(destination.availableBytes)} free`
		: destination.mountPath
			? "capacity unavailable"
			: "not mounted";
	return `${name} · ${destination.devicePath} · ${destination.filesystem.toUpperCase()} · ${available}`;
}

function titleCase(value: string) {
	return value
		.split("_")
		.map((part) => part.charAt(0).toUpperCase() + part.slice(1))
		.join(" ");
}

export function RecoveryImagePlanner({
	assessment,
}: {
	assessment: RecoveryAssessment;
}) {
	const router = useRouter();
	const [destinations, setDestinations] = useState<RecoveryDestination[]>([]);
	const [selected, setSelected] = useState<RecoveryDestination | null>(null);
	const [plan, setPlan] = useState<RecoveryImagePlan | null>(null);
	const [loading, setLoading] = useState(true);
	const [mounting, setMounting] = useState(false);
	const [planning, setPlanning] = useState(false);
	const [starting, setStarting] = useState(false);
	const [error, setError] = useState<string | null>(null);

	const loadDestinations = useCallback(async () => {
		setLoading(true);
		setError(null);
		setPlan(null);
		try {
			const records = await getRecoveryDestinations(assessment.devicePath);
			setDestinations(records);
			setSelected((current) => {
				if (current) {
					const refreshed = records.find(
						(candidate) => destinationKey(candidate) === destinationKey(current),
					);
					if (refreshed) return refreshed;
				}
				return records.at(0) ?? null;
			});
		} catch (loadError) {
			setError(
				loadError instanceof Error
					? loadError.message
					: "Failed to discover recovery destinations.",
			);
		} finally {
			setLoading(false);
		}
	}, [assessment.devicePath]);

	useEffect(() => {
		void loadDestinations();
	}, [loadDestinations]);

	const replaceDestination = (destination: RecoveryDestination) => {
		setSelected(destination);
		setDestinations((current) =>
			current.map((candidate) =>
				destinationKey(candidate) === destinationKey(destination)
					? destination
					: candidate,
			),
		);
	};

	const mountDestination = async () => {
		if (!selected || !assessment.identity) return;
		setMounting(true);
		setError(null);
		setPlan(null);
		try {
			const mounted = await mountRecoveryDestination(
				assessment.devicePath,
				assessment.identity,
				selected,
			);
			replaceDestination(mounted);
		} catch (mountError) {
			setError(
				mountError instanceof Error
					? mountError.message
					: "Failed to mount the recovery destination.",
			);
		} finally {
			setMounting(false);
		}
	};

	const buildPlan = async () => {
		if (!selected || !assessment.identity) return;
		setPlanning(true);
		setError(null);
		setPlan(null);
		try {
			setPlan(
				await planRecoveryImage(
					assessment.devicePath,
					assessment.identity,
					selected,
				),
			);
		} catch (planError) {
			setError(
				planError instanceof Error
					? planError.message
					: "Failed to build the recovery image plan.",
			);
		} finally {
			setPlanning(false);
		}
	};

	const startImage = async () => {
		if (!selected || !assessment.identity || plan?.decision !== "ready") return;
		setStarting(true);
		setError(null);
		try {
			const started = await startRecoveryImage(
				assessment.devicePath,
				assessment.identity,
				selected,
			);
			router.push(
				`/?tab=recovery&recoveryJobId=${encodeURIComponent(started.jobId)}`,
			);
		} catch (startError) {
			setError(
				startError instanceof Error
					? startError.message
					: "Failed to start the recovery image.",
			);
		} finally {
			setStarting(false);
		}
	};

	return (
		<div className="space-y-4 rounded-md border p-4">
			<div className="flex flex-wrap items-start justify-between gap-3">
				<div>
					<h3 className="flex items-center gap-2 font-semibold">
						<HardDrive className="h-4 w-4" />
						Recovery image destination
					</h3>
					<p className="mt-1 text-sm text-muted-foreground">
						Choose a separate removable drive for the source image and
						resumable map file.
					</p>
				</div>
				<Button
					variant="ghost"
					size="sm"
					onClick={() => void loadDestinations()}
					disabled={loading || mounting || planning || starting}
				>
					<RefreshCw className={`mr-2 h-4 w-4 ${loading ? "animate-spin" : ""}`} />
					Refresh
				</Button>
			</div>

			<Alert>
				<Save className="h-4 w-4" />
				<AlertDescription>
					DZap excludes the source and live boot drive. Mounting affects only
					the selected destination; planning does not create an image or write
					files.
				</AlertDescription>
			</Alert>

			{error && (
				<Alert className="border-destructive/50 bg-destructive/5">
					<XCircle className="h-4 w-4 text-destructive" />
					<AlertDescription className="text-destructive">{error}</AlertDescription>
				</Alert>
			)}

			{!loading && destinations.length === 0 ? (
				<p className="text-sm text-muted-foreground">
					No eligible destination was found. Attach another USB drive with an
					ext4, exFAT, or FAT32 filesystem, then refresh.
				</p>
			) : (
				<Select
					value={selected ? destinationKey(selected) : undefined}
					onValueChange={(value) => {
						const destination = destinations.find(
							(candidate) => destinationKey(candidate) === value,
						);
						if (destination) {
							setSelected(destination);
							setPlan(null);
							setError(null);
						}
					}}
					disabled={loading || destinations.length === 0}
				>
					<SelectTrigger>
						<SelectValue placeholder={loading ? "Finding destinations..." : "Select destination"} />
					</SelectTrigger>
					<SelectContent>
						{destinations.map((destination) => (
							<SelectItem
								key={destinationKey(destination)}
								value={destinationKey(destination)}
							>
								{destinationLabel(destination)}
							</SelectItem>
						))}
					</SelectContent>
				</Select>
			)}

			{selected && (
				<div className="space-y-3">
					<div className="grid gap-3 text-sm sm:grid-cols-3">
						<div className="rounded-md bg-muted/40 p-3">
							<p className="text-xs text-muted-foreground">Filesystem</p>
							<p className="font-medium">{selected.filesystem.toUpperCase()}</p>
						</div>
						<div className="rounded-md bg-muted/40 p-3">
							<p className="text-xs text-muted-foreground">Mount state</p>
							<p className="font-medium">{selected.mountPath ? "Mounted" : "Not mounted"}</p>
						</div>
						<div className="rounded-md bg-muted/40 p-3">
							<p className="text-xs text-muted-foreground">Available space</p>
							<p className="font-medium">{formatBytes(selected.availableBytes)}</p>
						</div>
					</div>

					<div className="flex flex-wrap gap-2">
						{!selected.mountPath && (
							<Button
								variant="outline"
								onClick={() => void mountDestination()}
								disabled={mounting || planning || starting}
							>
								{mounting && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
								Mount destination
							</Button>
						)}
						<Button
							onClick={() => void buildPlan()}
							disabled={!selected.mountPath || mounting || planning || starting}
						>
							{planning && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
							Build image plan
						</Button>
					</div>
				</div>
			)}

			{plan && (
				<div className="space-y-3" aria-live="polite">
					<div className="flex flex-wrap items-center gap-2">
						<Badge
							className={
								plan.decision === "ready"
									? "bg-success/20 text-success"
									: "bg-destructive/20 text-destructive"
							}
						>
							{plan.decision === "ready" ? "Ready to image" : "Blocked"}
						</Badge>
						<span className="text-sm text-muted-foreground">
							{formatBytes(plan.imageSizeBytes)} image + {formatBytes(plan.reserveBytes)} reserve
						</span>
					</div>
					{plan.outputDirectory && (
						<p className="break-all text-sm text-muted-foreground">
							Planned directory: {plan.outputDirectory}
						</p>
					)}
					<div className="space-y-2">
						{plan.checks.map((check) => (
							<div
								key={check.code}
								className={`flex gap-2 rounded-md border p-3 text-sm ${
									check.status === "passed"
										? "border-success/40 bg-success/10 text-success"
										: "border-destructive/40 bg-destructive/10 text-destructive"
								}`}
							>
								{check.status === "passed" ? (
									<CheckCircle2 className="mt-0.5 h-4 w-4 shrink-0" />
								) : (
									<XCircle className="mt-0.5 h-4 w-4 shrink-0" />
								)}
								<div>
									<p className="font-medium">{titleCase(check.code)}</p>
									<p>{check.message}</p>
								</div>
							</div>
						))}
					</div>
					{plan.decision === "ready" && (
						<div className="space-y-2 rounded-md border border-warning/40 bg-warning/5 p-3">
							<p className="text-sm text-muted-foreground">
								DZap will read the source and write a sparse image, map, log,
								and job record to the selected destination. The source remains
								unmounted.
							</p>
							<Button
								onClick={() => void startImage()}
								disabled={starting}
							>
								{starting ? (
									<Loader2 className="mr-2 h-4 w-4 animate-spin" />
								) : (
									<Save className="mr-2 h-4 w-4" />
								)}
								Start recovery image
							</Button>
						</div>
					)}
				</div>
			)}
		</div>
	);
}
