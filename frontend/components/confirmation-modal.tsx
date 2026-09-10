"use client";

import { useState } from "react";
import { AlertTriangle, Shield, Trash2, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Checkbox } from "@/components/ui/checkbox";
import {
	Dialog,
	DialogContent,
	DialogDescription,
	DialogFooter,
	DialogHeader,
	DialogTitle,
} from "@/components/ui/dialog";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";
import type { Device, StorageDevice } from "@/lib/types";

interface ConfirmationModalProps {
	isOpen: boolean;
	onClose: () => void;
	onConfirm: () => void;
	device: Device | null;
	wipeMethod: string;
	title?: string; // Added optional title prop
	description?: string; // Added optional description prop
	confirmText?: string; // Added optional confirm button text
	isDestructive?: boolean;
}

export function ConfirmationModal({
	isOpen,
	onClose,
	onConfirm,
	device,
	wipeMethod,
	title = "Confirm Data Destruction", // Default title with override option
	description = "This action will permanently destroy all data on the selected device. This operation cannot be undone.", // Default description with override option
	confirmText = "Confirm Wipe", // Default confirm text with override option
	isDestructive = true,
}: ConfirmationModalProps) {
	const [confirmationText, setConfirmationText] = useState("");
	const [hasAcceptedWarning, setHasAcceptedWarning] = useState(false);
	const [hasConfirmedSerial, setHasConfirmedSerial] = useState(false);

	const serial =
		device?.deviceCategory === "mobile"
			? device.serial
			: (device as StorageDevice)?.serial ||
				(device as StorageDevice)?.name;
	const requiredText = device ? `WIPE ${serial}` : "";
	const isConfirmationValid = confirmationText === requiredText;
	const canProceed =
		isConfirmationValid && hasAcceptedWarning && hasConfirmedSerial;

	const handleConfirm = () => {
		if (canProceed) {
			onConfirm();
			handleClose();
		}
	};

	const handleClose = () => {
		setConfirmationText("");
		setHasAcceptedWarning(false);
		setHasConfirmedSerial(false);
		onClose();
	};

	if (!device) return null;

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

	return (
		<Dialog open={isOpen} onOpenChange={handleClose}>
			<DialogContent className="max-w-2xl">
				<DialogHeader>
					<DialogTitle className="flex items-center space-x-2 text-destructive">
						<AlertTriangle className="h-6 w-6" />
						<span>{title}</span>
					</DialogTitle>
					<DialogDescription>{description}</DialogDescription>
				</DialogHeader>

				<div className="space-y-6">
					{/* Device Information */}
					<div className="bg-destructive/5 border border-destructive/20 rounded-lg p-4">
						<h3 className="font-semibold text-foreground mb-3 flex items-center space-x-2">
							<Shield className="h-5 w-5" />
							<span>Target Device</span>
						</h3>
						<div className="grid grid-cols-2 gap-4 text-sm">
							<div>
								<label className="text-muted-foreground">
									Device Name
								</label>
								<p className="font-medium text-foreground">
									{device.name}
								</p>
							</div>
							<div>
								<label className="text-muted-foreground">
									Type
								</label>
								<p className="font-medium text-foreground">
									{device.type}
								</p>
							</div>
							<div>
								<label className="text-muted-foreground">
									Model
								</label>
								<p className="font-medium text-foreground">
									{device.model}
								</p>
							</div>
							{device.deviceCategory === "storage" && (
								<div>
									<label className="text-muted-foreground">
										Capacity
									</label>
									<p className="font-medium text-foreground">
										{formatBytes(device.size)}
									</p>
								</div>
							)}
							<div className="col-span-2">
								<label className="text-muted-foreground">
									Serial Number
								</label>
								<p className="font-mono font-medium text-foreground bg-muted px-2 py-1 rounded">
									{serial}
								</p>
							</div>
						</div>
					</div>

					{/* Wipe Method */}
					<div className="bg-muted/50 rounded-lg p-4">
						<h3 className="font-semibold text-foreground mb-2">
							Wipe Configuration
						</h3>
						<div className="flex items-center space-x-2">
							<Badge
								variant="outline"
								className="bg-primary/10 text-primary border-primary/20"
							>
								{wipeMethod}
							</Badge>
						</div>
					</div>

					{/* Warnings */}
					<Alert className="border-destructive/50 bg-destructive/5">
						<AlertTriangle className="h-4 w-4 text-destructive" />
						<AlertDescription className="text-destructive">
							<strong>WARNING:</strong> This operation will
							permanently destroy all data on this device. Data
							recovery will be impossible after this process
							completes. Ensure you have backed up any important
							data before proceeding.
						</AlertDescription>
					</Alert>

					{/* Confirmation Checkboxes */}
					<div className="space-y-4">
						<div className="flex items-start space-x-3">
							<Checkbox
								id="accept-warning"
								checked={hasAcceptedWarning}
								onCheckedChange={(checked) =>
									setHasAcceptedWarning(checked as boolean)
								}
							/>
							<Label
								htmlFor="accept-warning"
								className="text-sm leading-relaxed"
							>
								I understand that this will permanently destroy
								all data on the device and that data recovery
								will be impossible.
							</Label>
						</div>

						<div className="flex items-start space-x-3">
							<Checkbox
								id="confirm-serial"
								checked={hasConfirmedSerial}
								onCheckedChange={(checked) =>
									setHasConfirmedSerial(checked as boolean)
								}
							/>
							<Label
								htmlFor="confirm-serial"
								className="text-sm leading-relaxed"
							>
								I have verified the device serial number and
								confirm this is the correct device to wipe:{" "}
								<span className="font-mono font-medium">
									{serial}
								</span>
							</Label>
						</div>
					</div>

					{/* Confirmation Text Input */}
					<div className="space-y-2">
						<Label
							htmlFor="confirmation-text"
							className="text-sm font-medium"
						>
							Type{" "}
							<span className="font-mono bg-muted px-1 rounded">
								{requiredText}
							</span>{" "}
							to confirm:
						</Label>
						<Input
							id="confirmation-text"
							value={confirmationText}
							onChange={(e) =>
								setConfirmationText(e.target.value)
							}
							placeholder={`Type "${requiredText}" here`}
							className={cn(
								"font-mono",
								confirmationText &&
									!isConfirmationValid &&
									"border-destructive focus:ring-destructive",
							)}
						/>
						{confirmationText && !isConfirmationValid && (
							<p className="text-sm text-destructive">
								Confirmation text does not match. Please type
								exactly: {requiredText}
							</p>
						)}
					</div>
				</div>

				<DialogFooter className="flex space-x-2">
					<Button variant="outline" onClick={handleClose}>
						<X className="h-4 w-4 mr-2" />
						Cancel
					</Button>
					<Button
						variant="destructive"
						onClick={handleConfirm}
						disabled={!canProceed}
						className="bg-destructive hover:bg-destructive/90"
					>
						<Trash2 className="h-4 w-4 mr-2" />
						{confirmText}
					</Button>
				</DialogFooter>
			</DialogContent>
		</Dialog>
	);
}
