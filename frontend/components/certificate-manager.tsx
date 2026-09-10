"use client";

import { useEffect, useMemo, useState } from "react";
import { CheckCircle, Download, Eye, FileText, Search, Shield } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
	Card,
	CardContent,
	CardDescription,
	CardHeader,
	CardTitle,
} from "@/components/ui/card";
import {
	Dialog,
	DialogContent,
	DialogDescription,
	DialogHeader,
	DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import {
	Table,
	TableBody,
	TableCell,
	TableHead,
	TableHeader,
	TableRow,
} from "@/components/ui/table";
import { downloadCertificatePdf, getCertificates } from "@/lib/utils";
import type { SignedCertificate } from "@/lib/types";

function formatDate(value: string) {
	return new Date(value).toLocaleString();
}

function formatBytes(value: string) {
	const bytes = Number(value);
	if (!Number.isFinite(bytes) || bytes <= 0) return value || "Unknown";
	const units = ["B", "KiB", "MiB", "GiB", "TiB"];
	const index = Math.min(
		Math.floor(Math.log(bytes) / Math.log(1024)),
		units.length - 1,
	);
	return `${(bytes / 1024 ** index).toFixed(index === 0 ? 0 : 2)} ${units[index]}`;
}

function saveBlob(blob: Blob, fileName: string) {
	const url = URL.createObjectURL(blob);
	const anchor = document.createElement("a");
	anchor.href = url;
	anchor.download = fileName;
	document.body.appendChild(anchor);
	anchor.click();
	anchor.remove();
	URL.revokeObjectURL(url);
}

export function CertificateManager() {
	const [certificates, setCertificates] = useState<SignedCertificate[]>([]);
	const [selected, setSelected] = useState<SignedCertificate | null>(null);
	const [searchTerm, setSearchTerm] = useState("");
	const [loading, setLoading] = useState(true);
	const [error, setError] = useState<string | null>(null);
	const [downloadingJobId, setDownloadingJobId] = useState<string | null>(null);

	useEffect(() => {
		let cancelled = false;
		getCertificates()
			.then((records) => {
				if (!cancelled) setCertificates(records);
			})
			.catch((loadError) => {
				if (!cancelled) {
					setError(
						loadError instanceof Error
							? loadError.message
							: "Failed to load certificates.",
					);
				}
			})
			.finally(() => {
				if (!cancelled) setLoading(false);
			});
		return () => {
			cancelled = true;
		};
	}, []);

	const filteredCertificates = useMemo(() => {
		const query = searchTerm.trim().toLowerCase();
		if (!query) return certificates;
		return certificates.filter(({ data }) =>
			[
				data.jobId,
				data.devicePath,
				data.deviceModel,
				data.deviceSerial,
				data.deviceWwn,
				data.wipeMethod,
			].some((value) => value.toLowerCase().includes(query)),
		);
	}, [certificates, searchTerm]);

	const downloadJson = (certificate: SignedCertificate) => {
		saveBlob(
			new Blob([JSON.stringify(certificate, null, 2)], {
				type: "application/json",
			}),
			`certificate-${certificate.data.jobId}.json`,
		);
	};

	const downloadPdf = async (certificate: SignedCertificate) => {
		setError(null);
		setDownloadingJobId(certificate.data.jobId);
		try {
			const pdf = await downloadCertificatePdf(certificate.data.jobId);
			saveBlob(pdf, `certificate-${certificate.data.jobId}.pdf`);
		} catch (downloadError) {
			setError(
				downloadError instanceof Error
					? downloadError.message
					: "Failed to download certificate PDF.",
			);
		} finally {
			setDownloadingJobId(null);
		}
	};

	return (
		<div className="space-y-6">
			<div>
				<h1 className="text-2xl font-bold text-foreground">
					Certificate Management
				</h1>
				<p className="text-muted-foreground">
					Signed certificates issued from verified server wipe jobs
				</p>
			</div>

			<Card className="component-border component-border-hover">
				<CardContent className="pt-6">
					<div className="relative">
						<Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
						<Input
							className="pl-10"
							placeholder="Search by job, device, serial, WWN, or method..."
							value={searchTerm}
							onChange={(event) => setSearchTerm(event.target.value)}
						/>
					</div>
					{error && <p className="mt-3 text-sm text-destructive">{error}</p>}
				</CardContent>
			</Card>

			<Card className="component-border component-border-hover">
				<CardHeader>
					<CardTitle className="flex items-center gap-2">
						<FileText className="h-5 w-5" />
						Certificates
						<Badge variant="secondary">{filteredCertificates.length}</Badge>
					</CardTitle>
					<CardDescription>
						Each record is signed by this DZap installation and bound to a
						wipe evidence hash.
					</CardDescription>
				</CardHeader>
				<CardContent>
					{loading ? (
						<p className="py-8 text-center text-sm text-muted-foreground">
							Loading certificates...
						</p>
					) : filteredCertificates.length === 0 ? (
						<p className="py-8 text-center text-sm text-muted-foreground">
							No matching certificates have been issued.
						</p>
					) : (
						<Table>
							<TableHeader>
								<TableRow>
									<TableHead>Job</TableHead>
									<TableHead>Device</TableHead>
									<TableHead>Method</TableHead>
									<TableHead>Completed</TableHead>
									<TableHead>Status</TableHead>
									<TableHead className="text-right">Actions</TableHead>
								</TableRow>
							</TableHeader>
							<TableBody>
								{filteredCertificates.map((certificate) => (
									<TableRow key={certificate.data.jobId}>
										<TableCell className="font-mono text-xs">
											{certificate.data.jobId}
										</TableCell>
										<TableCell>
											<p className="font-medium">
												{certificate.data.deviceModel}
											</p>
											<p className="text-xs text-muted-foreground">
												{certificate.data.deviceSerial ||
													certificate.data.devicePath}
											</p>
										</TableCell>
										<TableCell>{certificate.data.wipeMethod}</TableCell>
										<TableCell>{formatDate(certificate.data.completedAt)}</TableCell>
										<TableCell>
											<Badge className="bg-success/20 text-success">
												<CheckCircle className="mr-1 h-3 w-3" /> Verified
											</Badge>
										</TableCell>
										<TableCell>
											<div className="flex justify-end gap-2">
												<Button
													variant="ghost"
													size="icon"
													onClick={() => setSelected(certificate)}
													aria-label="View certificate"
												>
													<Eye className="h-4 w-4" />
												</Button>
												<Button
													variant="ghost"
													size="icon"
													onClick={() => downloadJson(certificate)}
													aria-label="Download signed JSON"
												>
													<Download className="h-4 w-4" />
												</Button>
											</div>
										</TableCell>
									</TableRow>
								))}
							</TableBody>
						</Table>
					)}
				</CardContent>
			</Card>

			<Dialog open={selected !== null} onOpenChange={(open) => !open && setSelected(null)}>
				<DialogContent className="max-h-[85vh] max-w-3xl overflow-y-auto component-border">
					{selected && (
						<>
							<DialogHeader>
								<DialogTitle className="flex items-center gap-2">
									<Shield className="h-5 w-5" />
									Signed Data Destruction Certificate
								</DialogTitle>
								<DialogDescription className="font-mono text-xs">
									{selected.data.jobId}
								</DialogDescription>
							</DialogHeader>

							<div className="grid gap-4 text-sm sm:grid-cols-2">
								<Detail label="Device path" value={selected.data.devicePath} mono />
								<Detail label="Model" value={selected.data.deviceModel} />
								<Detail label="Serial" value={selected.data.deviceSerial || "Not reported"} mono />
								<Detail label="WWN" value={selected.data.deviceWwn || "Not reported"} mono />
								<Detail label="Capacity" value={formatBytes(selected.data.deviceSizeBytes)} />
								<Detail label="Transport" value={selected.data.deviceTransport || "Not reported"} />
								<Detail label="Wipe method" value={selected.data.wipeMethod} />
								<Detail label="Started" value={formatDate(selected.data.startedAt)} />
								<Detail label="Completed" value={formatDate(selected.data.completedAt)} />
								<Detail label="Issued" value={formatDate(selected.data.timestamp)} />
								<Detail
									label="Verification strategy"
									value={selected.data.verification.strategy}
								/>
								<Detail
									label="Bytes checked"
									value={selected.data.verification.bytesChecked.toLocaleString()}
								/>
								<Detail
									label="Identity revalidated"
									value={selected.data.verification.identityRevalidated ? "Yes" : "No"}
								/>
								{selected.data.verification.expectedPattern && (
									<Detail
										label="Expected pattern"
										value={selected.data.verification.expectedPattern}
									/>
								)}
							</div>

							<Detail
								label="Readback SHA-256"
								value={selected.data.verification.readbackSha256}
								mono
								wrap
							/>
							{selected.data.verification.firmwareStatusSha256 && (
								<Detail
									label="Firmware status SHA-256"
									value={selected.data.verification.firmwareStatusSha256}
									mono
									wrap
								/>
							)}
							<Detail label="Evidence hash" value={selected.data.evidenceHash} mono wrap />
							<Detail label="RSA signature" value={selected.signature} mono wrap />
							<Detail label="Public key" value={selected.publicKey} mono wrap />

							<div className="flex flex-wrap justify-end gap-2">
								<Button variant="outline" onClick={() => downloadJson(selected)}>
									<Download className="mr-2 h-4 w-4" /> JSON
								</Button>
								<Button
									onClick={() => downloadPdf(selected)}
									disabled={downloadingJobId === selected.data.jobId}
								>
									<FileText className="mr-2 h-4 w-4" />
									{downloadingJobId === selected.data.jobId
										? "Preparing PDF..."
										: "Download PDF"}
								</Button>
							</div>
						</>
					)}
				</DialogContent>
			</Dialog>
		</div>
	);
}

function Detail({
	label,
	value,
	mono = false,
	wrap = false,
}: {
	label: string;
	value: string;
	mono?: boolean;
	wrap?: boolean;
}) {
	return (
		<div className="space-y-1">
			<p className="text-xs text-muted-foreground">{label}</p>
			<p
				className={`${mono ? "font-mono text-xs" : "font-medium"} ${wrap ? "break-all whitespace-pre-wrap" : ""}`}
			>
				{value}
			</p>
		</div>
	);
}
