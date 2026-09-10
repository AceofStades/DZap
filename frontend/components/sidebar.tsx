"use client";

import { useState } from "react";
import { RefreshCw, HardDrive, Smartphone, UsbIcon } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Separator } from "@/components/ui/separator";
import { cn } from "@/lib/utils";
import type { TabType } from "@/app/page";
import type { Device, StorageDevice, MobileDevice } from "@/lib/types";

interface SidebarProps {
	activeTab: TabType;
	selectedDevice: Device | null;
	onSelectDevice: (device: Device) => void;
	devices: Device[];
	onRefresh: () => void;
}

export function Sidebar({
	activeTab,
	selectedDevice,
	onSelectDevice,
	devices,
	onRefresh,
}: SidebarProps) {
	const [isRefreshing, setIsRefreshing] = useState(false);

	const handleRefresh = async () => {
		setIsRefreshing(true);
		await onRefresh();
		setIsRefreshing(false);
	};

	const getDeviceIcon = (type: Device["type"]) => {
		switch (type) {
			case "SATA SSD":
			case "NVMe SSD":
				return <HardDrive className="h-4 w-4" />;
			case "USB Drive":
				return <UsbIcon className="h-4 w-4" />;
			case "Android":
				return <Smartphone className="h-4 w-4" />;
			default:
				return <HardDrive className="h-4 w-4" />;
		}
	};

	const formatBytes = (bytes: string, decimals = 2) => {
		const bytesNum = parseInt(bytes, 10);
		if (bytesNum === 0) return "0 Bytes";
		const k = 1024;
		const dm = decimals < 0 ? 0 : decimals;
		const sizes = ["Bytes", "KB", "MB", "GB", "TB", "PB", "EB", "ZB", "YB"];
		const i = Math.floor(Math.log(bytesNum) / Math.log(k));
		return (
			parseFloat((bytesNum / Math.pow(k, i)).toFixed(dm)) + " " + sizes[i]
		);
	};

	const getStatusColor = (status: Device["status"]) => {
		switch (status) {
			case "ready":
				return "border-success bg-success/10 text-success";
			case "wiping":
				return "border-warning bg-warning/10 text-warning";
			case "completed":
				return "border-success bg-success/10 text-success";
			case "error":
				return "border-destructive bg-destructive/10 text-destructive";
			case "not-ready":
				return "border-destructive bg-destructive/10 text-destructive";
			default:
				return "border-muted bg-muted/10 text-muted-foreground";
		}
	};

	const getStatusBadgeColor = (status: Device["status"]) => {
		switch (status) {
			case "ready":
				return "bg-success/20 text-success hover:bg-success/30";
			case "wiping":
				return "bg-warning/20 text-warning hover:bg-warning/30";
			case "completed":
				return "bg-success/20 text-success hover:bg-success/30";
			case "error":
				return "bg-destructive/20 text-destructive hover:bg-destructive/30";
			case "not-ready":
				return "bg-destructive/20 text-destructive hover:bg-destructive/30";
			default:
				return "bg-muted/20 text-muted-foreground hover:bg-muted/30";
		}
	};

	if (activeTab !== "devices") {
		return null;
	}

	return (
		<aside className="w-80 bg-sidebar border-r border-sidebar-border p-4">
			<div className="space-y-4">
				<div className="flex items-center justify-between">
					<h2 className="text-lg font-semibold text-sidebar-foreground">
						Devices
					</h2>
					<Button
						variant="ghost"
						size="sm"
						onClick={handleRefresh}
						disabled={isRefreshing}
						className="text-sidebar-foreground hover:bg-sidebar-accent"
					>
						<RefreshCw
							className={cn(
								"h-4 w-4",
								isRefreshing && "animate-spin",
							)}
						/>
					</Button>
				</div>

				<div className="space-y-2 max-h-[calc(100vh-200px)] overflow-y-auto">
					{devices.map((device, index) => (
						<div key={device.id}>
							<Card
								className={cn(
									"p-4 cursor-pointer transition-all duration-200 hover:shadow-md",
									{
										"border-blue-600 bg-blue-600/10 text-blue-600":
											device.deviceCategory ===
												"storage" && device.isOSDrive,
										[getStatusColor(device.status)]:
											!(
												device.deviceCategory ===
													"storage" && device.isOSDrive
											),
									},
									selectedDevice?.id === device.id &&
										"ring-2 ring-primary",
								)}
								onClick={() => onSelectDevice(device)}
							>
								<div className="space-y-2">
									<div className="flex items-center justify-between">
										<div className="flex items-center space-x-2">
											{getDeviceIcon(device.type)}
											<span className="font-medium text-sm">
												{device.name}
											</span>
										</div>
										<Badge
											className={cn(
												device.deviceCategory ===
													"storage" &&
													device.isOSDrive
													? "bg-blue-600/20 text-blue-600"
													: getStatusBadgeColor(
															device.status,
														),
											)}
										>
											{device.deviceCategory ===
												"storage" && device.isOSDrive
												? "OS Drive"
												: device.status || "unknown"}
										</Badge>
									</div>

									<div className="text-xs text-muted-foreground space-y-1">
										<div>{device.model}</div>
										<div className="flex justify-between">
											<span>{device.type}</span>
											<span>
												{device.deviceCategory ===
												"storage"
													? formatBytes(device.size)
													: ""}
											</span>
										</div>
									</div>

									{device.status === "wiping" && (
										<div className="w-full bg-muted rounded-full h-1.5">
											<div
												className="bg-warning h-1.5 rounded-full animate-pulse"
												style={{ width: "45%" }} // This should be dynamic based on wipe progress
											/>
										</div>
									)}
								</div>
							</Card>
							{device.deviceCategory === "storage" &&
								device.isOSDrive &&
								index < devices.length - 1 && (
									<Separator className="my-2" />
								)}
						</div>
					))}
				</div>
			</div>
		</aside>
	);
}
