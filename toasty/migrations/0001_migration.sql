CREATE TABLE "blob_objects" (
    "path" TEXT NOT NULL,
    "ref_count" INTEGER NOT NULL,
    "created_at" TIMESTAMPTZ(6) NOT NULL,
    "tombstoned_at" TIMESTAMPTZ(6),
    PRIMARY KEY ("path")
);
