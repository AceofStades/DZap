"use client";

import { Shield, RotateCcw, ArrowLeft, Database, Search, HardDrive, FileCheck2, Sun, Moon } from "lucide-react";
import { useTheme } from "next-themes";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent } from "@/components/ui/card";

interface DataRecoveryViewProps {
	onBackToModeSelect: () => void;
	onSwitchToWiping: () => void;
}

export function DataRecoveryView({ onBackToModeSelect, onSwitchToWiping }: DataRecoveryViewProps) {
	const { theme, setTheme } = useTheme();

	return (
		<div className="flex flex-col h-screen bg-background text-foreground overflow-hidden">
			{/* Top Header - Consistent with DZAP Pro layout */}
			<header className="sticky top-0 z-50 bg-card border-b border-border shadow-sm">
				<div className="flex items-center justify-between px-6 py-4">
					<div className="flex items-center space-x-6">
						{/* Mode Switcher Button */}
						<Button
							variant="ghost"
							size="sm"
							onClick={onBackToModeSelect}
							className="text-muted-foreground hover:text-foreground flex items-center space-x-2 px-3 border border-border/60 hover:bg-accent"
							title="Return to Mode Selection"
						>
							<ArrowLeft className="h-4 w-4" />
							<span className="text-xs font-semibold">Change Mode</span>
						</Button>

						{/* Brand & Suite Badge */}
						<div className="flex items-center space-x-3">
							<div className="flex items-center space-x-2">
								<Shield className="h-6 w-6 text-emerald-500" />
								<span className="text-xl font-bold tracking-tight text-foreground">DZAP Pro</span>
							</div>
							<Badge variant="outline" className="border-emerald-500/30 text-emerald-600 dark:text-emerald-400 bg-emerald-500/10 text-xs font-medium">
								Data Recovery Suite
							</Badge>
						</div>

						{/* Dummy / Upcoming Navigation Tabs to match current layout */}
						<nav className="hidden md:flex space-x-1 pl-4 border-l border-border/60">
							<button className="px-3.5 py-1.5 text-xs font-medium rounded-md bg-emerald-500 text-white shadow-sm">
								Scanner
							</button>
							<button className="px-3.5 py-1.5 text-xs font-medium rounded-md text-muted-foreground hover:bg-accent hover:text-accent-foreground transition-colors">
								Recovered Files
							</button>
							<button className="px-3.5 py-1.5 text-xs font-medium rounded-md text-muted-foreground hover:bg-accent hover:text-accent-foreground transition-colors">
								Disk Images
							</button>
						</nav>
					</div>

					<div className="flex items-center space-x-4">
						<div className="hidden sm:flex items-center space-x-2 px-3 py-1.5 bg-emerald-500/10 text-emerald-500 rounded-full text-xs font-medium border border-emerald-500/20">
							<div className="w-2 h-2 bg-emerald-500 rounded-full animate-pulse" />
							<span>Recovery Engine Idle</span>
						</div>

						<Button
							variant="outline"
							size="sm"
							onClick={() => setTheme(theme === "light" ? "dark" : "light")}
							className="w-9 h-9 p-0"
						>
							<Sun className="h-4 w-4 rotate-0 scale-100 transition-all dark:-rotate-90 dark:scale-0" />
							<Moon className="absolute h-4 w-4 rotate-90 scale-0 transition-all dark:rotate-0 dark:scale-100" />
							<span className="sr-only">Toggle theme</span>
						</Button>
					</div>
				</div>
			</header>

			{/* Center of Screen: Data Recovery View */}
			<main className="flex-1 flex items-center justify-center p-6 bg-gradient-to-b from-background to-muted/20">
				<Card className="max-w-xl w-full border-2 border-border/80 shadow-2xl bg-card/80 backdrop-blur-sm text-center p-8 sm:p-10 relative overflow-hidden">
					{/* Glowing decorative gradient */}
					<div className="absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 w-64 h-64 bg-emerald-500/10 rounded-full blur-3xl pointer-events-none" />

					<CardContent className="space-y-6 relative z-10 p-0">
						{/* Icon in glowing ring */}
						<div className="mx-auto w-20 h-20 rounded-3xl bg-emerald-500/10 border-2 border-emerald-500/30 flex items-center justify-center text-emerald-500 shadow-lg shadow-emerald-500/10 animate-pulse">
							<RotateCcw className="h-10 w-10" />
						</div>

						{/* Centered Heading: "Data recovery" */}
						<div className="space-y-2">
							<Badge variant="outline" className="text-xs uppercase tracking-widest text-emerald-600 dark:text-emerald-400 border-emerald-500/30">
								Module Active
							</Badge>
							<h1 className="text-3xl sm:text-4xl font-extrabold tracking-tight text-foreground">
								Data recovery
							</h1>
							<p className="text-muted-foreground text-sm sm:text-base max-w-md mx-auto">
								The forensic salvage and filesystem reconstruction module. Designed to recover deleted files, rebuild lost partitions, and extract damaged storage volumes.
							</p>
						</div>

						{/* Status / Feature Pills */}
						<div className="grid grid-cols-2 gap-3 max-w-md mx-auto text-left pt-2">
							<div className="flex items-center space-x-2.5 p-3 rounded-xl bg-muted/50 border border-border/60 text-xs">
								<Search className="h-4 w-4 text-emerald-500 shrink-0" />
								<div>
									<p className="font-semibold text-foreground">Deep File Carving</p>
									<p className="text-muted-foreground text-[11px]">Signature-based recovery</p>
								</div>
							</div>
							<div className="flex items-center space-x-2.5 p-3 rounded-xl bg-muted/50 border border-border/60 text-xs">
								<HardDrive className="h-4 w-4 text-emerald-500 shrink-0" />
								<div>
									<p className="font-semibold text-foreground">Partition Recovery</p>
									<p className="text-muted-foreground text-[11px]">FAT, NTFS, ext4, APFS</p>
								</div>
							</div>
						</div>

						{/* Navigation and Actions */}
						<div className="flex flex-col sm:flex-row items-center justify-center gap-3 pt-4 border-t border-border/60">
							<Button
								variant="outline"
								onClick={onBackToModeSelect}
								className="w-full sm:w-auto text-xs font-semibold flex items-center justify-center space-x-2"
							>
								<ArrowLeft className="h-3.5 w-3.5" />
								<span>Return to Mode Selection</span>
							</Button>

							<Button
								variant="default"
								onClick={onSwitchToWiping}
								className="w-full sm:w-auto bg-amber-600 hover:bg-amber-700 text-white text-xs font-semibold"
							>
								<span>Switch to Data Wiping</span>
							</Button>
						</div>
					</CardContent>
				</Card>
			</main>
		</div>
	);
}
