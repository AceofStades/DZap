"use client";

import { useState, useEffect } from "react";
import {
	AlertTriangle,
	Shield,
	Zap,
	Settings,
	Play,
	Square,
	HardDrive,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import {
	Card,
	CardContent,
	CardDescription,
	CardHeader,
	CardTitle,
} from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Progress } from "@/components/ui/progress";
import {
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
} from "@/components/ui/select";
import { Separator } from "@/components/ui/separator";

import { cn } from "@/lib/utils";
import { ConfirmationModal } from "@/components/confirmation-modal";
import { useRouter } from "next/navigation";
import type {
	Device,
	StorageDevice,
	DriveHealth,
	WipeMethod,
	WipeRequest,
	WipePlan,
} from "@/lib/types";
import {
	getDriveHealth,
	preflightWipe,
	startWipe,
	getWipeMethods,
	unmountDevice,
} from "@/lib/utils";

interface DeviceManagerProps {
	selectedDevice: Device | null;
	onDeviceUpdate: () => void;
}

export function DeviceManager({
	selectedDevice,
	onDeviceUpdate,
}: DeviceManagerProps) {
	const [wipeMethod, setWipeMethod] = useState("overwrite_1_pass");
	const [showAdvanced, setShowAdvanced] = useState(false);
	const [showConfirmation, setShowConfirmation] = useState(false);
	const [showStopConfirmation, setShowStopConfirmation] = useState(false);
	const [isPreflighting, setIsPreflighting] = useState(false);
	const [preflightError, setPreflightError] = useState<string | null>(null);
	const [approvedPlan, setApprovedPlan] = useState<WipePlan | null>(null);
	const [health, setHealth] = useState<DriveHealth | null>(null);
	const [availableWipeMethods, setAvailableWipeMethods] = useState<
		WipeMethod[]
	>([]);
	const router = useRouter();

	const device = selectedDevice;

	useEffect(() => {
		const fetchHealth = async () => {
			if (device && device.deviceCategory === "storage") {
				try {
					const healthData = await getDriveHealth(device.name);
					setHealth(healthData);
				} catch (error) {
					console.error("Failed to fetch drive health:", error);
					setHealth(null);
				}
			}
		};

		const fetchWipeMethods = async () => {
			if (device) {
				try {
					const methods = await getWipeMethods(device.id);
					setAvailableWipeMethods(methods);
					if (methods.length > 0) {
						setWipeMethod(methods[0].id);
					}
				} catch (error) {
					console.error("Failed to fetch wipe methods:", error);
					setAvailableWipeMethods([]);
				}
			}
		};

		fetchHealth();
		fetchWipeMethods();
	}, [device]);

	useEffect(() => {
		setApprovedPlan(null);
		setPreflightError(null);
	}, [device?.id, wipeMethod]);

	const buildWipeRequest = (target: Device): WipeRequest => ({
		DevicePath: target.id,
		Method: wipeMethod,
		DeviceSerial:
			target.deviceCategory === "mobile"
				? target.serial
				: (target as StorageDevice).name,
		DeviceType: target.type,
		DeviceModel: target.model,
	});

	const handleStartWipe = async () => {
		if (!device || device.status !== "ready") return;

		setIsPreflighting(true);
		setPreflightError(null);
		setApprovedPlan(null);
		try {
			const plan = await preflightWipe(buildWipeRequest(device));
			if (plan.decision === "blocked") {
				const reasons = plan.checks
					.filter((check) => check.status === "blocked")
					.map((check) => check.message);
				setPreflightError(
					reasons.join(" ") || "Wipe blocked by safety preflight.",
				);
				return;
			}
			setApprovedPlan(plan);
			setShowConfirmation(true);
		} catch (error) {
			setPreflightError(
				error instanceof Error
					? error.message
					: "Unable to run wipe safety checks.",
			);
		} finally {
			setIsPreflighting(false);
		}
	};

	const handleConfirmWipe = async () => {
		if (!device || !approvedPlan?.identity) {
			setShowConfirmation(false);
			setPreflightError(
				"The approved device identity is missing. Run the safety check again.",
			);
			return;
		}
		console.log("Starting wipe process for device:", device?.id);
		try {
			setPreflightError(null);
			const started = await startWipe({
				...buildWipeRequest(device),
				ExpectedIdentity: approvedPlan.identity,
			});
			setShowConfirmation(false);
			router.push(
				`/?tab=progress&jobId=${encodeURIComponent(started.jobId)}`,
			);
		} catch (error) {
			console.error("Failed to start wipe:", error);
			setShowConfirmation(false);
			setPreflightError(
				error instanceof Error
					? error.message
					: "Failed to start wipe process.",
			);
		}
	};

	const handleUnmountDevice = async () => {
		if (!device) return;
		console.log("Unmounting device:", device?.id);
		try {
			await unmountDevice(device.id);
			setTimeout(() => {
				onDeviceUpdate();
			}, 1000); // 1-second delay to allow OS to update mount status
		} catch (error) {
			console.error("Failed to unmount device:", error);
			// TODO: Show an error toast/message
		}
	};

	const handleStopWipe = () => {
		setShowStopConfirmation(true);
	};

	const handleConfirmStopWipe = () => {
		console.log("Stopping wipe process for device:", device?.id);
		setShowStopConfirmation(false);
		// In real app, this would call backend API to stop wiping
	};

	const formatBytes = (bytes: string, decimals = 2) => {
		const bytesNum = parseInt(bytes, 10);
		if (!bytesNum || bytesNum === 0) return "0 Bytes";
		const k = 1024;
		const dm = decimals < 0 ? 0 : decimals;
		const sizes = ["Bytes", "KB", "MB", "GB", "TB", "PB", "EB", "ZB", "YB"];
		const i = Math.floor(Math.log(bytesNum) / Math.log(k));
		return (
			parseFloat((bytesNum / Math.pow(k, i)).toFixed(dm)) + " " + sizes[i]
		);
	};

	if (!device) {
		return (
			<div className="flex items-center justify-center h-96">
				<div className="text-center space-y-4">
					<Shield className="h-16 w-16 text-muted-foreground mx-auto" />
					<div>
						<h3 className="text-lg font-medium text-foreground">
							No Device Selected
						</h3>
						<p className="text-muted-foreground">
							Select a device from the sidebar to view details and
							actions
						</p>
					</div>
				</div>
			</div>
		);
	}

	const isWiping = device.status === "wiping";
	const isCompleted = device.status === "completed";
	const isNotReady = device.status === "not-ready";
	const isSSD = device.type === "SATA SSD" || device.type === "NVMe SSD";
	const isReady = device.status === "ready";
	const isOSDrive = device.deviceCategory === "storage" && device.isOSDrive;

	return (
		<>
			<div className="space-y-6">
				<div className="flex items-center justify-between">
					<div>
						<h1 className="text-2xl font-bold text-foreground">
							Device Manager
						</h1>
						<p className="text-muted-foreground">
							Manage and wipe selected storage device
						</p>
					</div>
					<div className="flex items-center space-x-2">
						<Badge
							className={cn({
								"bg-blue-600/20 text-blue-600": isOSDrive,
								"bg-success/20 text-success":
									isReady && !isOSDrive,
								"bg-warning/20 text-warning": isWiping,
								"bg-destructive/20 text-destructive":
									isNotReady,
							})}
						>
							{isOSDrive
								? "OS DRIVE"
								: device.status?.toUpperCase() || "UNKNOWN"}
						</Badge>
					</div>
				</div>

				{isSSD && isReady && (
					<Alert className="component-border component-border-hover">
						<AlertTriangle className="h-4 w-4" />
						<AlertDescription>
							SSD detected. Crypto-erase or NVMe sanitize is
							recommended for optimal data destruction on
							solid-state drives.
						</AlertDescription>
					</Alert>
				)}

				{preflightError && (
					<Alert className="border-destructive/50 bg-destructive/5">
						<AlertTriangle className="h-4 w-4 text-destructive" />
						<AlertDescription className="text-destructive">
							{preflightError}
						</AlertDescription>
					</Alert>
				)}

				{isWiping && (
					// This part will be handled by the progress tracker page
					<Card className="border-warning bg-warning/5 component-border component-border-hover">
						<CardHeader>
							<CardTitle className="flex items-center space-x-2 text-warning">
								<Play className="h-5 w-5" />
								<span>Wipe in Progress</span>
							</CardTitle>
							<CardDescription>
								Data destruction is currently running on this
								device. See Progress tab for details.
							</CardDescription>
						</CardHeader>
					</Card>
				)}

				<div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
					<Card className="component-border component-border-hover">
						<CardHeader>
							<CardTitle className="flex items-center space-x-2">
								<Shield className="h-5 w-5" />
								<span>Device Information</span>
							</CardTitle>
							<CardDescription>
								Hardware specifications and identifiers
							</CardDescription>
						</CardHeader>
						<CardContent className="space-y-4">
							<div className="grid grid-cols-2 gap-4">
								<div>
									<label className="text-sm text-muted-foreground">
										Device Name
									</label>
									<p className="text-lg font-medium text-foreground">
										{device.name}
									</p>
								</div>
								<div>
									<label className="text-sm text-muted-foreground">
										Type
									</label>
									<p className="text-lg font-medium text-foreground">
										{device.type}
									</p>
								</div>
								<div>
									<label className="text-sm text-muted-foreground">
										Model
									</label>
									<p className="text-lg font-medium text-foreground">
										{device.model}
									</p>
								</div>
								{device.deviceCategory === "storage" && (
									<div>
										<label className="text-sm text-muted-foreground">
											Capacity
										</label>
										<p className="text-lg font-medium text-foreground">
											{formatBytes(device.size)}
										</p>
									</div>
								)}
								<div>
									<label className="text-sm text-muted-foreground">
										Serial Number
									</label>
									<p className="text-lg font-medium text-foreground font-mono">
										{device.deviceCategory === "mobile"
											? device.serial
											: device.serial || "N/A"}
									</p>
								</div>
								<div>
									<label className="text-sm text-muted-foreground">
										Firmware
									</label>
									<p className="text-lg font-medium text-foreground">
										{"N/A"}
									</p>
								</div>
							</div>

							<Separator />

							{device.deviceCategory === "storage" &&
								device.partitions && (
									<div>
										<label className="text-sm text-muted-foreground mb-2 block">
											Partitions
										</label>
										<div className="space-y-2">
											{device.partitions.map(
												(partition, index) => (
													<div
														key={index}
														className="flex justify-between items-center p-2 bg-muted rounded-md component-border"
													>
														<div>
															<span className="font-medium text-sm text-foreground">
																{partition.name}
															</span>
															<span className="text-xs text-muted-foreground ml-2">
																(
																{partition.type ||
																	"unknown"}
																)
															</span>
														</div>
														<span className="text-sm font-medium text-foreground">
															{formatBytes(
																partition.size,
															)}
														</span>
													</div>
												),
											)}
										</div>
									</div>
								)}
						</CardContent>
					</Card>

					<Card className="component-border component-border-hover">
						<CardHeader>
							<CardTitle className="flex items-center space-x-2">
								<Zap className="h-5 w-5" />
								<span>Health & SMART Data</span>
							</CardTitle>
							<CardDescription>
								Device health information and diagnostics
							</CardDescription>
						</CardHeader>
						<CardContent className="space-y-4">
							{health ? (
								<div className="grid grid-cols-2 gap-4">
									<div>
										<label className="text-sm text-muted-foreground">
											Overall Health
										</label>
										<div className="flex items-center space-x-2">
											<Badge
												className={
													health.predictedStatus ===
													"Healthy"
														? "bg-success/20 text-success"
														: "bg-warning/20 text-warning"
												}
											>
												{health.predictedStatus}
											</Badge>
										</div>
									</div>
									<div>
										<label className="text-sm text-muted-foreground">
											S.M.A.R.T. Status
										</label>
										<p className="text-lg font-medium text-foreground">
											{health.smartStatus}
										</p>
									</div>
									{health.failureProbability > 0 && (
										<div>
											<label className="text-sm text-muted-foreground">
												Failure Risk
											</label>
											<p className="text-lg font-medium text-foreground">
												{(
													health.failureProbability *
													100
												).toFixed(2)}
												%
											</p>
										</div>
									)}
									{health.smartAttributes &&
										Object.values(
											health.smartAttributes,
										).map((attr) => (
											<div key={attr.name}>
												<label className="text-sm text-muted-foreground">
													{attr.name}
												</label>
												<p className="text-lg font-medium text-foreground">
													{attr.value}
												</p>
											</div>
										))}
								</div>
							) : (
								<p className="text-muted-foreground text-sm">
									{device.deviceCategory === "storage"
										? "Loading health data..."
										: "Health data not available for this device type."}
								</p>
							)}
						</CardContent>
					</Card>
				</div>

				<Card
					className="component-border component-border-hover"
					aria-disabled={isOSDrive}
				>
					<CardHeader>
						<CardTitle className="flex items-center space-x-2">
							<Settings className="h-5 w-5" />
							<span>Wipe Configuration</span>
						</CardTitle>
						<CardDescription>
							{isOSDrive
								? "Wiping the OS drive is not permitted."
								: "Configure data destruction method and parameters"}
						</CardDescription>
					</CardHeader>
					<CardContent className="space-y-6">
						<div className="grid grid-cols-1 md:grid-cols-2 gap-4">
							<div className="space-y-2">
								<label className="text-sm font-medium">
									Wipe Method
								</label>
								<Select
									value={wipeMethod}
									onValueChange={setWipeMethod}
									disabled={isWiping || isOSDrive}
								>
									<SelectTrigger>
										<SelectValue />
									</SelectTrigger>
									<SelectContent>
										{availableWipeMethods.map((method) => (
											<SelectItem
												key={method.id}
												value={method.id}
											>
												{method.name}
											</SelectItem>
										))}
									</SelectContent>
								</Select>
							</div>

							<div className="space-y-2">
								<label className="text-sm font-medium">
									Verification
								</label>
								<Select
									defaultValue="basic"
									disabled={isWiping || isOSDrive}
								>
									<SelectTrigger>
										<SelectValue />
									</SelectTrigger>
									<SelectContent>
										<SelectItem value="none">
											No Verification
										</SelectItem>
										<SelectItem value="basic">
											Basic Verification
										</SelectItem>
										<SelectItem value="full">
											Full Read Verification
										</SelectItem>
									</SelectContent>
								</Select>
							</div>
						</div>

						<div className="flex items-center space-x-2">
							<Button
								variant="outline"
								size="sm"
								onClick={() => setShowAdvanced(!showAdvanced)}
								disabled={isWiping || isOSDrive}
							>
								<Settings className="h-4 w-4 mr-2" />
								{showAdvanced ? "Hide" : "Show"} Advanced
								Options
							</Button>
						</div>

						{showAdvanced && (
							<div className="space-y-4 p-4 bg-muted/50 rounded-lg">
								<div className="grid grid-cols-1 md:grid-cols-2 gap-4">
									<div className="space-y-2">
										<label className="text-sm font-medium">
											Block Size
										</label>
										<Select
											defaultValue="1mb"
											disabled={isWiping || isOSDrive}
										>
											<SelectTrigger>
												<SelectValue />
											</SelectTrigger>
											<SelectContent>
												<SelectItem value="64kb">
													64 KB
												</SelectItem>
												<SelectItem value="1mb">
													1 MB
												</SelectItem>
												<SelectItem value="4mb">
													4 MB
												</SelectItem>
												<SelectItem value="16mb">
													16 MB
												</SelectItem>
											</SelectContent>
										</Select>
									</div>

									<div className="space-y-2">
										<label className="text-sm font-medium">
											Thread Count
										</label>
										<Select
											defaultValue="auto"
											disabled={isWiping || isOSDrive}
										>
											<SelectTrigger>
												<SelectValue />
											</SelectTrigger>
											<SelectContent>
												<SelectItem value="1">
													1 Thread
												</SelectItem>
												<SelectItem value="2">
													2 Threads
												</SelectItem>
												<SelectItem value="4">
													4 Threads
												</SelectItem>
												<SelectItem value="auto">
													Auto
												</SelectItem>
											</SelectContent>
										</Select>
									</div>
								</div>
							</div>
						)}

						<Separator />

						<div className="flex flex-wrap gap-3">
							{isNotReady && !isOSDrive && (
								<Button
									size="lg"
									variant="destructive"
									onClick={handleUnmountDevice}
									className="bg-red-600 hover:bg-red-700"
								>
									<HardDrive className="h-4 w-4 mr-2" />
									Unmount Device
								</Button>
							)}

							<Button
								size="lg"
								disabled={!isReady || isOSDrive || isPreflighting}
								className="bg-primary hover:bg-primary/90"
								onClick={handleStartWipe}
							>
								<Play className="h-4 w-4 mr-2" />
								{isPreflighting
									? "Checking Safety..."
									: isWiping
									? "Wiping in Progress..."
									: isCompleted
										? "Wipe Completed"
										: "Start Wipe"}
							</Button>

							{!isReady && !isOSDrive && (
								<Button
									variant="outline"
									size="lg"
									disabled={isWiping}
								>
									<Shield className="h-4 w-4 mr-2" />
									Export Device Info
								</Button>
							)}
						</div>
					</CardContent>
				</Card>
			</div>

			<ConfirmationModal
				isOpen={showConfirmation}
				onClose={() => setShowConfirmation(false)}
				onConfirm={handleConfirmWipe}
				device={device as StorageDevice}
				wipeMethod={wipeMethod}
			/>

			<ConfirmationModal
				isOpen={showStopConfirmation}
				onClose={() => setShowStopConfirmation(false)}
				onConfirm={handleConfirmStopWipe}
				device={device as StorageDevice}
				wipeMethod="Stop Wipe Operation"
				title="Stop Wipe Operation"
				description="Wiped data will not be recovered now, do you still want to stop?"
				confirmText="Yes, Stop Wipe"
				isDestructive={true}
			/>
		</>
	);
}
