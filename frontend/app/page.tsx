"use client";

import { useCallback, useEffect, useState } from "react";
import { useRouter, useSearchParams } from "next/navigation";
import { Header } from "@/components/header";
import { Sidebar } from "@/components/sidebar";
import { DeviceManager } from "@/components/device-manager";
import { ProgressTracker } from "@/components/progress-tracker";
import { CertificateManager } from "@/components/certificate-manager";
import type { Device, StorageDevice, MobileDevice } from "@/lib/types";
import { getDevices } from "@/lib/utils";

export type TabType = "devices" | "progress" | "certificates";

const isTab = (value: string | null): value is TabType =>
	value === "devices" || value === "progress" || value === "certificates";

export default function Dashboard() {
	const router = useRouter();
	const searchParams = useSearchParams();
	const requestedTab = searchParams.get("tab");
	const activeTab: TabType = isTab(requestedTab) ? requestedTab : "devices";
	const [allDevices, setAllDevices] = useState<Device[]>([]);
	const [selectedDevice, setSelectedDevice] = useState<Device | null>(null);

	const setActiveTab = useCallback(
		(tab: TabType) => {
			const next = new URLSearchParams(searchParams.toString());
			next.set("tab", tab);
			if (tab !== "progress") next.delete("jobId");
			router.push(`/?${next.toString()}`);
		},
		[router, searchParams],
	);

	const fetchDevices = async () => {
		try {
			const { storage, mobile } = await getDevices();

			const storageWithCategory: StorageDevice[] = (storage || []).map(
				(d) => ({
					...d,
					id: d.name,
					deviceCategory: "storage",
					status: d.isMounted ? "not-ready" : "ready",
				}),
			);

			const mobileWithCategory: MobileDevice[] = (mobile || []).map(
				(d) => ({
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
		<div className="flex h-screen flex-col bg-background text-foreground">
			<Header
				activeTab={activeTab}
				onTabChange={setActiveTab}
				selectedDeviceName={selectedDevice?.name}
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
	);
}
