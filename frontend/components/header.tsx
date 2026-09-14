"use client";

import { Moon, Shield, Sun } from "lucide-react";
import { useTheme } from "next-themes";
import type { TabType } from "@/app/page";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

interface HeaderProps {
	activeTab: TabType;
	onTabChange: (tab: TabType) => void;
	selectedDeviceName?: string;
}

const tabs: Array<{ id: TabType; label: string }> = [
	{ id: "devices", label: "Devices" },
	{ id: "progress", label: "Wipe Progress" },
	{ id: "recovery", label: "Recovery" },
	{ id: "certificates", label: "Evidence" },
];

export function Header({
	activeTab,
	onTabChange,
	selectedDeviceName,
}: HeaderProps) {
	const { theme, setTheme } = useTheme();

	return (
		<header className="sticky top-0 z-50 border-b border-border bg-card shadow-sm">
			<div className="flex items-center justify-between px-6 py-4">
				<div className="flex items-center space-x-8">
					<div className="flex items-center space-x-2">
						<Shield className="h-6 w-6 text-primary" />
						<span className="text-xl font-semibold text-foreground">
							DZap Live
						</span>
					</div>

					<nav className="flex space-x-1" aria-label="Dashboard views">
						{tabs.map((tab) => (
							<button
								type="button"
								key={tab.id}
								onClick={() => onTabChange(tab.id)}
								className={cn(
									"rounded-md px-4 py-2 text-sm font-medium transition-colors",
									"hover:bg-accent hover:text-accent-foreground",
									activeTab === tab.id
										? "bg-primary text-primary-foreground"
										: "text-muted-foreground",
								)}
							>
								{tab.label}
							</button>
						))}
					</nav>
				</div>

				<div className="flex items-center space-x-4">
					{selectedDeviceName && (
						<div className="flex items-center space-x-2 rounded-full bg-muted px-3 py-1.5 text-sm text-muted-foreground">
							<Shield className="h-3.5 w-3.5" />
							<span>Selected: {selectedDeviceName}</span>
						</div>
					)}

					<Button
						variant="outline"
						size="sm"
						onClick={() => setTheme(theme === "light" ? "dark" : "light")}
						className="h-9 w-9 p-0"
					>
						<Sun className="h-4 w-4 rotate-0 scale-100 transition-all dark:-rotate-90 dark:scale-0" />
						<Moon className="absolute h-4 w-4 rotate-90 scale-0 transition-all dark:rotate-0 dark:scale-100" />
						<span className="sr-only">Toggle theme</span>
					</Button>
				</div>
			</div>
		</header>
	);
}
