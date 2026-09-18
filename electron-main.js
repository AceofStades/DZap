const { app, BrowserWindow } = require("electron");

// Linux sandbox compatibility
app.commandLine.appendSwitch("no-sandbox");
app.commandLine.appendSwitch("disable-gpu-sandbox");

function createWindow() {
	const mainWindow = new BrowserWindow({
		width: 1366,
		height: 850,
		minWidth: 1024,
		minHeight: 700,
		title: "DZAP Pro - Data Wiping & Forensic Carving Suite",
		autoHideMenuBar: true,
		webPreferences: {
			nodeIntegration: false,
			contextIsolation: true,
		},
	});

	// Wait briefly or retry if dev server is warming up
	const targetUrl = process.env.DZAP_UI_URL || "http://localhost:3000";
	mainWindow.loadURL(targetUrl);
}

app.whenReady().then(createWindow);

app.on("window-all-closed", () => {
	if (process.platform !== "darwin") {
		app.quit();
	}
});
