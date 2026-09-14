"use client";

import { useEffect, useRef, useState } from "react";
import {
	AlertTriangle,
	CheckCircle2,
	CircleHelp,
	Search,
	XCircle,
} from "lucide-react";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
	Card,
	CardContent,
	CardDescription,
	CardHeader,
	CardTitle,
} from "@/components/ui/card";
import type {
	RecoveryAssessment,
	RecoveryCheckStatus,
	StorageDevice,
} from "@/lib/types";
import { assessRecovery } from "@/lib/utils";
import { RecoveryImagePlanner } from "@/components/recovery-image-plan";

interface RecoveryAssessmentProps {
	device: StorageDevice;
}

const checkStyle: Record<RecoveryCheckStatus, string> = {
	passed: "border-success/40 bg-success/10 text-success",
	warning: "border-warning/40 bg-warning/10 text-warning",
	blocked: "border-destructive/40 bg-destructive/10 text-destructive",
	unknown: "border-muted-foreground/30 bg-muted text-muted-foreground",
};

const decisionStyle: Record<RecoveryAssessment["decision"], string> = {
	ready: "bg-success/20 text-success",
	caution: "bg-warning/20 text-warning",
	blocked: "bg-destructive/20 text-destructive",
};

const contentLabels: Record<RecoveryAssessment["contentState"], string> = {
	structured_data: "Structured data found",
	non_blank: "Non-blank, unclassified",
	likely_blank: "Likely blank",
	unknown: "Unknown",
};

function StatusIcon({ status }: { status: RecoveryCheckStatus }) {
	switch (status) {
		case "passed":
			return <CheckCircle2 className="mt-0.5 h-4 w-4 shrink-0" />;
		case "warning":
			return <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" />;
		case "blocked":
			return <XCircle className="mt-0.5 h-4 w-4 shrink-0" />;
		case "unknown":
			return <CircleHelp className="mt-0.5 h-4 w-4 shrink-0" />;
	}
}

function titleCase(value: string) {
	return value
		.split("_")
		.map((part) => part.charAt(0).toUpperCase() + part.slice(1))
		.join(" ");
}

export function RecoveryAssessmentPanel({
	device,
}: RecoveryAssessmentProps) {
	const [assessment, setAssessment] = useState<RecoveryAssessment | null>(
		null,
	);
	const [loading, setLoading] = useState(false);
	const [error, setError] = useState<string | null>(null);
	const assessmentRun = useRef(0);

	useEffect(() => {
		assessmentRun.current += 1;
		setAssessment(null);
		setError(null);
		setLoading(false);
	}, [device.id]);

	const runAssessment = async () => {
		const run = assessmentRun.current + 1;
		assessmentRun.current = run;
		setLoading(true);
		setError(null);
		setAssessment(null);
		try {
			const result = await assessRecovery(device.id);
			if (assessmentRun.current === run) setAssessment(result);
		} catch (assessmentError) {
			if (assessmentRun.current === run) {
				setError(
					assessmentError instanceof Error
						? assessmentError.message
						: "Unable to assess this recovery source.",
				);
			}
		} finally {
			if (assessmentRun.current === run) setLoading(false);
		}
	};

	return (
		<Card className="component-border component-border-hover">
			<CardHeader>
				<div className="flex flex-wrap items-start justify-between gap-3">
					<div>
						<CardTitle className="flex items-center gap-2">
							<Search className="h-5 w-5" />
							Recovery Assessment
						</CardTitle>
						<CardDescription className="mt-1">
							Inspect signatures, encryption, bad-sector indicators, and
							sparse content samples before choosing a recovery method.
						</CardDescription>
					</div>
					{assessment && (
						<Badge className={decisionStyle[assessment.decision]}>
							{assessment.decision === "ready"
								? "Ready to plan"
								: titleCase(assessment.decision)}
						</Badge>
					)}
				</div>
			</CardHeader>
			<CardContent className="space-y-5">
				<Alert>
					<CircleHelp className="h-4 w-4" />
					<AlertDescription>
						This assessment opens the source read-only. It does not mount,
						repair, decrypt, or write to the drive. Sparse reads cannot prove
						that every sector is healthy or securely wiped.
					</AlertDescription>
				</Alert>

				{error && (
					<Alert className="border-destructive/50 bg-destructive/5">
						<AlertTriangle className="h-4 w-4 text-destructive" />
						<AlertDescription className="text-destructive">
							{error}
						</AlertDescription>
					</Alert>
				)}

				<Button onClick={runAssessment} disabled={loading} variant="outline">
					<Search className="mr-2 h-4 w-4" />
					{loading ? "Assessing source..." : "Assess Recoverability"}
				</Button>

				{assessment && (
					<div className="space-y-5" aria-live="polite">
						<div className="grid gap-3 sm:grid-cols-3">
							<div className="rounded-md border bg-muted/30 p-3">
								<p className="text-xs text-muted-foreground">Content</p>
								<p className="font-medium">
									{contentLabels[assessment.contentState]}
								</p>
							</div>
							<div className="rounded-md border bg-muted/30 p-3">
								<p className="text-xs text-muted-foreground">Media</p>
								<p className="font-medium">
									{titleCase(assessment.mediaCondition)}
								</p>
							</div>
							<div className="rounded-md border bg-muted/30 p-3">
								<p className="text-xs text-muted-foreground">Encryption</p>
								<p className="font-medium">
									{titleCase(assessment.encryption)}
								</p>
							</div>
						</div>

						<div className="space-y-2">
							<h3 className="text-sm font-semibold">Checks</h3>
							{assessment.checks.map((check) => (
								<div
									key={check.code}
									className={`flex gap-2 rounded-md border p-3 text-sm ${checkStyle[check.status]}`}
								>
									<StatusIcon status={check.status} />
									<div>
										<p className="font-medium">{titleCase(check.code)}</p>
										<p className="text-current/90">{check.message}</p>
									</div>
								</div>
							))}
						</div>

						{assessment.signatures.length > 0 && (
							<div className="space-y-2">
								<h3 className="text-sm font-semibold">Detected signatures</h3>
								<div className="flex flex-wrap gap-2">
									{assessment.signatures.map((signature) => (
										<Badge
											key={`${signature.devicePath}-${signature.kind}-${signature.value}`}
											variant="outline"
										>
											{signature.devicePath}: {signature.value}
										</Badge>
									))}
								</div>
							</div>
						)}

						<div className="space-y-2">
							<h3 className="text-sm font-semibold">Recommended next steps</h3>
							<ol className="list-decimal space-y-1 pl-5 text-sm text-muted-foreground">
								{assessment.recommendations.map((recommendation) => (
									<li key={recommendation}>{recommendation}</li>
								))}
							</ol>
						</div>

						{assessment.decision !== "blocked" && assessment.identity && (
							<RecoveryImagePlanner assessment={assessment} />
						)}
					</div>
				)}
			</CardContent>
		</Card>
	);
}
