"use client";

import { Shield, Flame, RotateCcw, CheckCircle2, ArrowRight, Sun, Moon, HardDrive, Cpu, Lock } from "lucide-react";
import { useTheme } from "next-themes";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";

interface ModeSelectorProps {
	onSelectMode: (mode: "wiping" | "recovery") => void;
}

export function ModeSelector({ onSelectMode }: ModeSelectorProps) {
	const { theme, setTheme } = useTheme();

	return (
		<div className="min-h-screen bg-background text-foreground flex flex-col justify-between selection:bg-primary/20">
			{/* Top Navbar */}
			<header className="border-b border-border bg-card/60 backdrop-blur-md sticky top-0 z-50">
				<div className="max-w-6xl mx-auto px-6 py-4 flex items-center justify-between">
					<div className="flex items-center space-x-3">
						<div className="w-10 h-10 rounded-xl bg-primary/10 border border-primary/20 flex items-center justify-center text-primary shadow-sm">
							<Shield className="h-5 w-5" />
						</div>
						<div>
							<div className="flex items-center space-x-2">
								<span className="text-xl font-bold tracking-tight text-foreground">DZAP Pro</span>
								<Badge variant="outline" className="text-[10px] px-1.5 py-0 uppercase tracking-widest border-primary/30 text-primary font-mono">
									Storage Suite
								</Badge>
							</div>
							<p className="text-xs text-muted-foreground">Certified Storage Sanitization & Recovery</p>
						</div>
					</div>

					<div className="flex items-center space-x-3">
						<div className="hidden sm:flex items-center space-x-2 px-3 py-1 bg-muted/60 border border-border rounded-full text-xs text-muted-foreground">
							<div className="w-2 h-2 rounded-full bg-emerald-500 animate-pulse" />
							<span>Appliance Ready</span>
						</div>

						<Button
							variant="outline"
							size="sm"
							onClick={() => setTheme(theme === "light" ? "dark" : "light")}
							className="w-9 h-9 p-0 rounded-lg border-border"
						>
							<Sun className="h-4 w-4 rotate-0 scale-100 transition-all dark:-rotate-90 dark:scale-0" />
							<Moon className="absolute h-4 w-4 rotate-90 scale-0 transition-all dark:rotate-0 dark:scale-100" />
							<span className="sr-only">Toggle theme</span>
						</Button>
					</div>
				</div>
			</header>

			{/* Main Center Selector */}
			<main className="flex-1 flex flex-col items-center justify-center px-6 py-12">
				<div className="max-w-4xl w-full text-center mb-10 space-y-3">
					<Badge variant="secondary" className="px-3 py-1 text-xs font-medium tracking-wide uppercase">
						Step 1 &bull; Initialize Operation
					</Badge>
					<h1 className="text-3xl sm:text-4xl lg:text-5xl font-extrabold tracking-tight text-foreground">
						Select Operation Mode
					</h1>
					<p className="text-muted-foreground text-base sm:text-lg max-w-2xl mx-auto">
						Choose whether you wish to perform irreversible cryptographic sanitization or initiate non-destructive forensic data recovery.
					</p>
				</div>

				<div className="max-w-4xl w-full grid grid-cols-1 md:grid-cols-2 gap-6 lg:gap-8">
					{/* Option 1: Data Wiping */}
					<Card className="relative group overflow-hidden border-2 border-border hover:border-amber-500/50 dark:hover:border-amber-400/50 transition-all duration-300 shadow-md hover:shadow-xl hover:shadow-amber-500/5 flex flex-col justify-between bg-gradient-to-b from-card to-card/70">
						<div className="absolute top-0 right-0 w-32 h-32 bg-amber-500/10 rounded-full blur-3xl pointer-events-none group-hover:bg-amber-500/20 transition-all" />
						
						<CardHeader className="space-y-4 pb-4">
							<div className="flex items-center justify-between">
								<div className="w-14 h-14 rounded-2xl bg-amber-500/10 border border-amber-500/20 flex items-center justify-center text-amber-500 shadow-inner group-hover:scale-105 transition-transform duration-300">
									<Flame className="h-7 w-7" />
								</div>
								<Badge variant="outline" className="border-amber-500/30 text-amber-600 dark:text-amber-400 bg-amber-500/5 text-xs font-semibold">
									Destructive
								</Badge>
							</div>

							<div>
								<CardTitle className="text-2xl font-bold tracking-tight text-foreground group-hover:text-amber-500 transition-colors">
									1. Data Wiping
								</CardTitle>
								<CardDescription className="text-sm mt-1 text-muted-foreground">
									NIST SP 800-88 & DoD 5220.22-M Compliant Erasure
								</CardDescription>
							</div>
						</CardHeader>

						<CardContent className="space-y-4 text-sm">
							<p className="text-muted-foreground leading-relaxed">
								Irreversibly sanitize NVMe, SATA SSDs, HDDs, and mobile devices with hardware-level firmware commands, multi-pass overwrites, and signed PDF verification certificates.
							</p>

							<div className="space-y-2 pt-2 border-t border-border/60">
								<div className="flex items-center space-x-2 text-xs text-foreground/90 font-medium">
									<CheckCircle2 className="h-4 w-4 text-amber-500 shrink-0" />
									<span>NVMe Sanitize & ATA Secure Erase</span>
								</div>
								<div className="flex items-center space-x-2 text-xs text-foreground/90 font-medium">
									<CheckCircle2 className="h-4 w-4 text-amber-500 shrink-0" />
									<span>Multi-pass NIST / DoD Overwrites</span>
								</div>
								<div className="flex items-center space-x-2 text-xs text-foreground/90 font-medium">
									<CheckCircle2 className="h-4 w-4 text-amber-500 shrink-0" />
									<span>RSA-Signed PDF Evidence Certificates</span>
								</div>
							</div>
						</CardContent>

						<CardFooter className="pt-4">
							<Button 
								className="w-full group/btn bg-gradient-to-r from-amber-600 to-amber-500 hover:from-amber-700 hover:to-amber-600 text-white font-semibold shadow-md"
								size="lg"
								onClick={() => onSelectMode("wiping")}
							>
								<span>Enter Data Wiping</span>
								<ArrowRight className="h-4 w-4 ml-2 transition-transform group-hover/btn:translate-x-1" />
							</Button>
						</CardFooter>
					</Card>

					{/* Option 2: Data Recovery */}
					<Card className="relative group overflow-hidden border-2 border-border hover:border-emerald-500/50 dark:hover:border-emerald-400/50 transition-all duration-300 shadow-md hover:shadow-xl hover:shadow-emerald-500/5 flex flex-col justify-between bg-gradient-to-b from-card to-card/70">
						<div className="absolute top-0 right-0 w-32 h-32 bg-emerald-500/10 rounded-full blur-3xl pointer-events-none group-hover:bg-emerald-500/20 transition-all" />
						
						<CardHeader className="space-y-4 pb-4">
							<div className="flex items-center justify-between">
								<div className="w-14 h-14 rounded-2xl bg-emerald-500/10 border border-emerald-500/20 flex items-center justify-center text-emerald-500 shadow-inner group-hover:scale-105 transition-transform duration-300">
									<RotateCcw className="h-7 w-7" />
								</div>
								<Badge variant="outline" className="border-emerald-500/30 text-emerald-600 dark:text-emerald-400 bg-emerald-500/5 text-xs font-semibold">
									Non-Destructive
								</Badge>
							</div>

							<div>
								<CardTitle className="text-2xl font-bold tracking-tight text-foreground group-hover:text-emerald-500 transition-colors">
									2. Data Recovery
								</CardTitle>
								<CardDescription className="text-sm mt-1 text-muted-foreground">
									Forensic Salvage & Partition Reconstruction
								</CardDescription>
							</div>
						</CardHeader>

						<CardContent className="space-y-4 text-sm">
							<p className="text-muted-foreground leading-relaxed">
								Perform deep inspection on connected storage media to salvage lost documents, scan corrupted file systems, recover deleted partitions, and extract valuable data safely.
							</p>

							<div className="space-y-2 pt-2 border-t border-border/60">
								<div className="flex items-center space-x-2 text-xs text-foreground/90 font-medium">
									<CheckCircle2 className="h-4 w-4 text-emerald-500 shrink-0" />
									<span>Read-only non-destructive scanning</span>
								</div>
								<div className="flex items-center space-x-2 text-xs text-foreground/90 font-medium">
									<CheckCircle2 className="h-4 w-4 text-emerald-500 shrink-0" />
									<span>Raw partition & file carving</span>
								</div>
								<div className="flex items-center space-x-2 text-xs text-foreground/90 font-medium">
									<CheckCircle2 className="h-4 w-4 text-emerald-500 shrink-0" />
									<span>Corrupted volume reconstruction</span>
								</div>
							</div>
						</CardContent>

						<CardFooter className="pt-4">
							<Button 
								className="w-full group/btn bg-gradient-to-r from-emerald-600 to-emerald-500 hover:from-emerald-700 hover:to-emerald-600 text-white font-semibold shadow-md"
								size="lg"
								onClick={() => onSelectMode("recovery")}
							>
								<span>Enter Data Recovery</span>
								<ArrowRight className="h-4 w-4 ml-2 transition-transform group-hover/btn:translate-x-1" />
							</Button>
						</CardFooter>
					</Card>
				</div>
			</main>

			{/* Footer Info Bar */}
			<footer className="border-t border-border/60 bg-card/30 py-4 px-6 text-xs text-muted-foreground">
				<div className="max-w-6xl mx-auto flex flex-col sm:flex-row items-center justify-between gap-2">
					<div className="flex items-center space-x-4">
						<span className="flex items-center space-x-1.5">
							<HardDrive className="h-3.5 w-3.5" />
							<span>Raw Storage Engine</span>
						</span>
						<span>&bull;</span>
						<span className="flex items-center space-x-1.5">
							<Cpu className="h-3.5 w-3.5" />
							<span>ArchISO / Linux Runtime</span>
						</span>
					</div>

					<div className="flex items-center space-x-1.5">
						<Lock className="h-3.5 w-3.5 text-primary" />
						<span>Identity-Bound Safety Protocol Active</span>
					</div>
				</div>
			</footer>
		</div>
	);
}
