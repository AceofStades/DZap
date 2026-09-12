import type React from "react";
import type { Metadata, Viewport } from "next";
import { GeistSans } from "geist/font/sans";
import { GeistMono } from "geist/font/mono";
import { Suspense } from "react";
import { ThemeProvider } from "@/components/theme-provider";
import "./globals.css";

export const metadata: Metadata = {
	title: "DZap Live USB",
	description:
		"Bootable storage sanitization appliance with device safety checks, verification, and signed evidence export.",
	keywords:
		"bootable USB, storage sanitization, secure erase, data destruction, evidence certificate, SSD sanitize",
	authors: [{ name: "DZap Project" }],
	creator: "DZap Project",
	publisher: "DZap Project",
	robots: { index: false, follow: false },
	applicationName: "DZap Live USB",
};

export const viewport: Viewport = {
	width: "device-width",
	initialScale: 1,
	themeColor: [
		{ media: "(prefers-color-scheme: light)", color: "#f5f7fa" },
		{ media: "(prefers-color-scheme: dark)", color: "#0a0a0a" },
	],
};

export default function RootLayout({
	children,
}: Readonly<{
	children: React.ReactNode;
}>) {
	return (
		<html lang="en" suppressHydrationWarning>
			<body
				className={`font-sans ${GeistSans.variable} ${GeistMono.variable}`}
			>
				<ThemeProvider
					attribute="class"
					defaultTheme="dark"
					enableSystem
					disableTransitionOnChange
				>
					<Suspense fallback={null}>{children}</Suspense>
				</ThemeProvider>
			</body>
		</html>
	);
}
