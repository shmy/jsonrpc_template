CREATE TABLE "files" (
    "path" TEXT NOT NULL,
    "ref_count" INTEGER NOT NULL,
    "created_at" TIMESTAMPTZ(6) NOT NULL,
    "tombstoned_at" TIMESTAMPTZ(6),
    PRIMARY KEY ("path")
);
CREATE TABLE "accounts" (
    "id" BIGINT NOT NULL,
    "name" TEXT NOT NULL,
    "phone" TEXT NOT NULL,
    "password" TEXT NOT NULL,
    "privileged" BOOLEAN NOT NULL,
    "avatar" TEXT,
    "remark" TEXT,
    "version" BIGINT NOT NULL,
    "created_at" TIMESTAMPTZ(6) NOT NULL,
    "updated_at" TIMESTAMPTZ(6) NOT NULL,
    PRIMARY KEY ("id")
);
CREATE UNIQUE INDEX "index_accounts_by_phone" ON "accounts" ("phone");
