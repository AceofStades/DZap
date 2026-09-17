"use client";

import { useState, useEffect } from "react";
import { Header } from "@/components/header";
import { Sidebar } from "@/components/sidebar";
import { DeviceManager } from "@/components/device-manager";
import { ProgressTracker } from "@/components/progress-tracker";
import { CertificateManager } from "@/components/certificate-manager";
import { ModeSelector } from "@/components/mode-selector";
import { DataRecoveryView } from "@/components/data-recovery-view";
import { ThemeProvider } from "@/components/theme-provider";
import type { Device, StorageDevice, MobileDevice } from "@/lib/types";
import { getDevices } from "@/lib/utils";

export type TabType = "devices" | "progress" | "certificates";
export type AppMode = "select" | "wiping" | "recovery";

export default function Dashboard() {
	const [appMode, setAppMode] = useState<AppMode>("select");
	const [activeTab, setActiveTab] = useState<TabType>("devices");
	const [allDevices, setAllDevices] = useState<Device[]>([]);
	const [selectedDevice, setSelectedDevice] = useState<Device | null>(null);

	const fetchDevices = async () => {
		try {
			const { storage, mobile } = await getDevices();

			const storageWithCategory: StorageDevice[] = (storage || []).map(
				(d: any) => ({
					...d,
					id: d.name,
					deviceCategory: "storage",
					status: d.isMounted ? "not-ready" : "ready",
				}),
			);

			const mobileWithCategory: MobileDevice[] = (mobile || []).map(
				(d: any) => ({
					...d,
					id: d.serial,
					deviceCategory: "mobile",
					status: "ready",
				}),
			);

			const combinedDevices = [
				...storageWithCategory,
				...mobileWithCategory,
			].sort((a, b) => {
				if (a.deviceCategory === "storage" && a.isOSDrive) return -1;
				if (b.deviceCategory === "storage" && b.isOSDrive) return 1;
				return 0;
			});
			setAllDevices(combinedDevices);

			setSelectedDevice((prevSelectedDevice) => {
				if (!prevSelectedDevice) {
					return combinedDevices.length > 0
						? combinedDevices[0]
						: null;
				}
				const updatedSelected = combinedDevices.find(
					(d) => d.id === prevSelectedDevice.id,
				);
				return (
					updatedSelected ||
					(combinedDevices.length > 0 ? combinedDevices[0] : null)
				);
			});
		} catch (error) {
			console.error("Failed to fetch devices:", error);
			setAllDevices([]);
			setSelectedDevice(null);
		}
	};

	useEffect(() => {
		fetchDevices();
	}, []);

	return (
		<ThemeProvider attribute="class" defaultTheme="dark" enableSystem>
			{appMode === "select" && (
				<ModeSelector onSelectMode={(mode) => setAppMode(mode)} />
			)}

			{appMode === "recovery" && (
				<DataRecoveryView
					onBackToModeSelect={() => setAppMode("select")}
					onSwitchToWiping={() => setAppMode("wiping")}
				/>
			)}

			{appMode === "wiping" && (
				<div className="flex flex-col h-screen bg-background text-foreground">
					<Header
						activeTab={activeTab}
						onTabChange={setActiveTab}
						selectedDeviceName={selectedDevice?.name}
						currentMode="wiping"
						onBackToModeSelect={() => setAppMode("select")}
					/>
					<div className="flex flex-1 overflow-hidden">
						<Sidebar
							devices={allDevices}
							selectedDevice={selectedDevice}
							onSelectDevice={setSelectedDevice}
							activeTab={activeTab}
							onRefresh={fetchDevices}
						/>
						<main className="flex-1 overflow-y-auto p-6">
							{activeTab === "devices" && (
								<DeviceManager
									selectedDevice={selectedDevice}
									onDeviceUpdate={fetchDevices}
								/>
							)}
							{activeTab === "progress" && <ProgressTracker />}
							{activeTab === "certificates" && <CertificateManager />}
						</main>
					</div>
				</div>
			)}
		</ThemeProvider>
	);
}
