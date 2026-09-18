"use client";

import React from "react";
import { Shield, ArrowLeft, Sun, Moon, Trash2 } from "lucide-react";
import { useTheme } from "next-themes";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { ForensicWorkspace } from "./forensics/forensic-workspace";

interface DataRecoveryViewProps {
	onBackToModeSelect: () => void;
	onSwitchToWiping: () => void;
}

export function DataRecoveryView({ onBackToModeSelect, onSwitchToWiping }: DataRecoveryViewProps) {
	const { theme, setTheme } = useTheme();

	return (
		<div className="flex flex-col h-screen bg-background text-foreground overflow-hidden">
			{/* Top Header - Unified DZAP Pro layout */}
			<header className="sticky top-0 z-50 bg-card/80 backdrop-blur-md border-b border-border/80 shadow-sm shrink-0">
				<div className="flex items-center justify-between px-6 py-3.5">
					<div className="flex items-center space-x-4">
						{/* Mode Switcher Button */}
						<Button
							variant="ghost"
							size="sm"
							onClick={onBackToModeSelect}
							className="text-muted-foreground hover:text-foreground flex items-center space-x-2 px-3 border border-border/60 hover:bg-accent h-8"
							title="Return to Mode Selection"
						>
							<ArrowLeft className="h-3.5 w-3.5" />
							<span className="text-xs font-semibold">Modes</span>
						</Button>

						{/* Brand & Suite Badge */}
						<div className="flex items-center space-x-3">
							<div className="flex items-center space-x-2">
								<Shield className="h-5 w-5 text-emerald-500" />
								<span className="text-lg font-bold tracking-tight text-foreground">DZAP Pro</span>
							</div>
							<Badge
								variant="outline"
								className="border-emerald-500/30 text-emerald-600 dark:text-emerald-400 bg-emerald-500/10 text-xs font-medium"
							>
								Forensic Carving & Recovery Suite
							</Badge>
						</div>
					</div>

					<div className="flex items-center space-x-3">
						{/* Quick Switch to Data Wiping */}
						<Button
							variant="outline"
							size="sm"
							onClick={onSwitchToWiping}
							className="text-xs text-muted-foreground hover:text-foreground hover:border-amber-500/50 flex items-center gap-1.5 h-8"
						>
							<Trash2 className="h-3.5 w-3.5 text-amber-500" />
							<span>Switch to Wiping</span>
						</Button>

						{/* Dark/Light Theme Toggle */}
						<Button
							variant="outline"
							size="sm"
							onClick={() => setTheme(theme === "light" ? "dark" : "light")}
							className="w-8 h-8 p-0"
							title="Toggle Dark/Light Mode"
						>
							<Sun className="h-3.5 w-3.5 rotate-0 scale-100 transition-all dark:-rotate-90 dark:scale-0 text-amber-500" />
							<Moon className="absolute h-3.5 w-3.5 rotate-90 scale-0 transition-all dark:rotate-0 dark:scale-100 text-blue-400" />
							<span className="sr-only">Toggle theme</span>
						</Button>
					</div>
				</div>
			</header>

			{/* Main Forensic Investigation Workspace */}
			<main className="flex-1 overflow-hidden flex flex-col">
				<ForensicWorkspace />
			</main>
		</div>
	);
}
