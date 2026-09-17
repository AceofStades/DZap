"use client"

import { Shield, Moon, Sun, ArrowLeft } from "lucide-react"
import { useTheme } from "next-themes"
import { cn } from "@/lib/utils"
import { Button } from "@/components/ui/button"
import type { TabType } from "@/app/page"

interface HeaderProps {
  activeTab: TabType
  onTabChange: (tab: TabType) => void
  selectedDeviceName?: string
  currentMode?: "wiping" | "recovery"
  onBackToModeSelect?: () => void
}

export function Header({ activeTab, onTabChange, selectedDeviceName, onBackToModeSelect }: HeaderProps) {
  const { theme, setTheme } = useTheme()

  const tabs = [
    { id: "devices" as TabType, label: "Device Manager", count: 3 },
    { id: "progress" as TabType, label: "Wipe Progress" },
    { id: "certificates" as TabType, label: "Certificates" },
  ]

  return (
    <header className="sticky top-0 z-50 bg-card border-b border-border shadow-sm">
      <div className="flex items-center justify-between px-6 py-4">
        <div className="flex items-center space-x-6">
          {onBackToModeSelect && (
            <Button
              variant="ghost"
              size="sm"
              onClick={onBackToModeSelect}
              className="text-muted-foreground hover:text-foreground flex items-center space-x-1.5 px-2.5 border border-border/60 hover:bg-accent"
              title="Return to Mode Selection"
            >
              <ArrowLeft className="h-4 w-4" />
              <span className="text-xs font-semibold hidden sm:inline">Change Mode</span>
            </Button>
          )}

          <div className="flex items-center space-x-2">
            <Shield className="h-6 w-6 text-primary" />
            <span className="text-xl font-semibold text-foreground">DZAP Pro</span>
          </div>

          <nav className="flex space-x-1">
            {tabs.map((tab) => (
              <button
                key={tab.id}
                onClick={() => onTabChange(tab.id)}
                className={cn(
                  "px-4 py-2 text-sm font-medium rounded-md transition-colors",
                  "hover:bg-accent hover:text-accent-foreground",
                  activeTab === tab.id ? "bg-primary text-primary-foreground" : "text-muted-foreground",
                )}
              >
                {tab.label}
                {tab.count && (
                  <span
                    className="
      ml-2 px-2 py-0.5 text-xs bg-muted rounded-full
      text-black dark:text-white
    "
                  >
                    {tab.count}
                  </span>
                )}
              </button>
            ))}
          </nav>
        </div>

        <div className="flex items-center space-x-4">
          {selectedDeviceName && (
            <div className="flex items-center space-x-2 px-3 py-1.5 bg-success/10 text-success rounded-full text-sm">
              <div className="w-2 h-2 bg-success rounded-full animate-pulse" />
              <span>{selectedDeviceName}</span>
            </div>
          )}

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
  )
}
