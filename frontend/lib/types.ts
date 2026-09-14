// A base interface with properties common to all devices
interface BaseDevice {
	id: string; // A unique identifier (path for storage, serial for mobile)
	model: string;
	type: string;
	name: string;
}

export interface Partition {
	name: string;
	size: string;
	type: string;
}

export interface BlockDependency {
	name: string;
	type: string;
}

// Specific type for standard storage drives
export interface StorageDevice extends BaseDevice {
	deviceCategory: "storage";
	serial: string;
	wwn: string;
	size: string;
	transport: string;
	majorMinor: string;
	isMounted: boolean;
	isFrozen: boolean;
	isOSDrive?: boolean;
	activeDependencies: BlockDependency[];
	partitions: Partition[];
	status?: "ready" | "wiping" | "completed" | "error" | "not-ready";
	health?: DriveHealth;
}
// Specific type for mobile devices
export interface MobileDevice extends BaseDevice {
	deviceCategory: "mobile";
	serial: string;
	status?: "ready" | "wiping" | "completed" | "error" | "not-ready";
}

// A single, unified type for any device in the app
export type Device = StorageDevice | MobileDevice;

export type DiscoveredStorageDevice = Omit<
	StorageDevice,
	"id" | "deviceCategory" | "status"
>;
export type DiscoveredMobileDevice = Omit<
	MobileDevice,
	"id" | "deviceCategory" | "status"
>;

export interface DeviceInventory {
	storage: DiscoveredStorageDevice[] | null;
	mobile: DiscoveredMobileDevice[] | null;
}

// --- Other types ---

export interface SmartAttribute {
	name: string;
	value: number;
}

export interface DriveHealth {
	predictedStatus: string;
	failureProbability: number;
	smartStatus: string;
	smartAttributes?: { [key: string]: SmartAttribute };
	temperature?: string;
	powerOnHours?: string;
	totalWrites?: string;
	wearLeveling?: string;
	badSectors?: string;
}

export interface WipeMethod {
	id: string;
	name: string;
	description: string;
}

export interface WipeRequest {
	DevicePath: string;
	Method: string;
	DeviceSerial: string;
	DeviceType: string;
	DeviceModel: string;
	ExpectedIdentity?: DeviceIdentity;
}

export interface DeviceIdentity {
	model: string;
	serial: string;
	wwn: string;
	sizeBytes: string;
	transport: string;
	majorMinor: string;
}

export type RecoveryCheckStatus =
	| "passed"
	| "warning"
	| "blocked"
	| "unknown";

export interface RecoveryCheck {
	code: string;
	status: RecoveryCheckStatus;
	message: string;
}

export interface RecoverySignature {
	devicePath: string;
	kind: "partition_table" | "filesystem";
	value: string;
}

export interface RecoverySmartEvidence {
	available: boolean;
	passed: boolean | null;
	reallocatedSectors: number | null;
	pendingSectors: number | null;
	offlineUncorrectable: number | null;
	reportedUncorrectable: number | null;
	nvmeMediaErrors: number | null;
	nvmeCriticalWarning: number | null;
}

export interface RecoveryContentSample {
	sourceOpened: boolean;
	requestedSamples: number;
	completedSamples: number;
	sampledBytes: number;
	zeroBytes: number;
	ffBytes: number;
	readErrors: string[];
}

export interface RecoveryAssessment {
	decision: "ready" | "caution" | "blocked";
	devicePath: string;
	deviceModel: string;
	deviceType: string;
	identity: DeviceIdentity | null;
	checks: RecoveryCheck[];
	signatures: RecoverySignature[];
	encryption: "not_detected" | "detected" | "unknown";
	mediaCondition: "healthy" | "degraded" | "unknown";
	contentState:
		| "structured_data"
		| "non_blank"
		| "likely_blank"
		| "unknown";
	smart: RecoverySmartEvidence;
	sample: RecoveryContentSample;
	recommendations: string[];
}

export interface RecoveryDestination {
	drivePath: string;
	devicePath: string;
	deviceMajorMinor: string;
	mountPath: string | null;
	filesystem: string;
	sizeBytes: string;
	driveIdentity: DeviceIdentity;
	filesystemSizeBytes: string | null;
	availableBytes: string | null;
	readOnly: boolean | null;
	capacityError: string | null;
}

export interface RecoveryPlanCheck {
	code: string;
	status: "passed" | "blocked";
	message: string;
}

export interface RecoveryImagePlan {
	decision: "ready" | "blocked";
	sourceDevicePath: string;
	sourceIdentity: DeviceIdentity | null;
	destination: RecoveryDestination | null;
	imageSizeBytes: string;
	reserveBytes: string;
	requiredBytes: string;
	outputDirectory: string | null;
	checks: RecoveryPlanCheck[];
}

export type RecoveryJobStatus =
	| "imaging"
	| "paused"
	| "image_complete"
	| "extracting"
	| "completed"
	| "cancelled"
	| "failed";

export interface RescueMapSummary {
	rescuedBytes: number;
	unreadableBytes: number;
	pendingBytes: number;
	totalBytes: number;
}

export interface RecoveryEvent {
	sequence: number;
	timestamp: string;
	eventType: string;
	message: string;
	previousHash: string | null;
	eventHash: string;
}

export interface RecoveryJobRecord {
	id: string;
	sourceDevicePath: string;
	sourceIdentity: DeviceIdentity;
	destination: RecoveryDestination;
	jobDirectory: string;
	imagePath: string;
	mapPath: string;
	logPath: string;
	status: RecoveryJobStatus;
	startedAt: string;
	updatedAt: string;
	completedAt: string | null;
	progressPercent: number;
	mapSummary: RescueMapSummary;
	lastMessage: string;
	failure: string | null;
	recoveryMethod: RecoveryMethod | null;
	recoveryOutputDirectory: string | null;
	recoveryResult: RecoveryResult | null;
	testdiskAnalysis: TestdiskAnalysis | null;
	evidenceHash: string;
	events: RecoveryEvent[];
}

export type RecoveryMethod = "filesystem_copy" | "photorec";

export interface RecoveryResult {
	method: RecoveryMethod;
	outputDirectory: string;
	manifestPath: string;
	manifestSha256: string;
	recoveredFileCount: number;
	recoveredBytes: number;
	skippedEntries: number;
}

export interface TestdiskAnalysis {
	completedAt: string;
	successful: boolean;
	logPath: string;
	logSha256: string;
	summary: string;
}

export interface RecoveryVolume {
	id: string;
	kind: "whole_disk" | "partition";
	sizeBytes: string;
	filesystem: string | null;
	label: string | null;
	encryption: "luks" | "bitlk" | null;
	filesystemCopySupported: boolean;
	photorecSupported: boolean;
}

export interface StartRecoveryResponse {
	status: string;
	jobId: string;
	deviceId: string;
}

export interface PreflightCheck {
	code: string;
	status: "passed" | "blocked";
	message: string;
}

export interface WipePlan {
	decision: "ready" | "blocked";
	devicePath: string;
	deviceModel: string;
	deviceType: string;
	method: string;
	identity: DeviceIdentity | null;
	checks: PreflightCheck[];
}

export interface StartWipeResponse {
	status: string;
	jobId: string;
	deviceId: string;
}

export interface EvidenceEvent {
	sequence: number;
	timestamp: string;
	eventType: string;
	message: string;
	previousHash: string | null;
	eventHash: string;
}

export interface WipeJobRecord {
	id: string;
	devicePath: string;
	deviceModel: string;
	deviceType: string;
	identity: DeviceIdentity;
	method: string;
	status: "running" | "verifying" | "verified" | "failed";
	startedAt: string;
	sanitizationCompletedAt: string | null;
	completedAt: string | null;
	failure: string | null;
	verification: VerificationResult | null;
	evidenceHash: string;
	events: EvidenceEvent[];
}

export interface VerificationResult {
	strategy:
		| "full_pattern_readback"
		| "ata_security_status_and_samples"
		| "nvme_format_status_and_samples"
		| "nvme_sanitize_status_and_samples";
	bytesChecked: number;
	readbackSha256: string;
	expectedPattern: string | null;
	firmwareStatusSha256: string | null;
	identityRevalidated: boolean;
}

export interface CertificateData {
	jobId: string;
	devicePath: string;
	deviceModel: string;
	deviceSerial: string;
	deviceWwn: string;
	deviceSizeBytes: string;
	deviceTransport: string;
	deviceMajorMinor: string;
	deviceType: string;
	wipeMethod: string;
	startedAt: string;
	completedAt: string;
	timestamp: string;
	verification: VerificationResult;
	evidenceHash: string;
}

export interface SignedCertificate {
	data: CertificateData;
	signature: string;
	publicKey: string;
}

export interface ExportDestination {
	drivePath: string;
	driveMajorMinor: string;
	devicePath: string;
	deviceMajorMinor: string;
	mountPath: string | null;
	model: string;
	serial: string;
	transport: string;
	filesystem: string;
	sizeBytes: string;
}

export interface EvidenceExportResult {
	jobId: string;
	bundlePath: string;
	exportedAt: string;
	keyFingerprintSha256: string;
	alreadyExisted: boolean;
	destination: ExportDestination;
}
