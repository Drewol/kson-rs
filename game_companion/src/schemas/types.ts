import { FromSchema } from "json-schema-to-ts";

const server = {
  $schema: "http://json-schema.org/draft-07/schema#",
  title: "GameState",
  oneOf: [
    {
      type: "object",
      required: ["variant"],
      properties: {
        variant: {
          type: "string",
          enum: ["None"],
        },
      },
    },
    {
      type: "object",
      required: ["variant"],
      properties: {
        variant: {
          type: "string",
          enum: ["TitleScreen"],
        },
      },
    },
    {
      type: "object",
      required: [
        "filters",
        "folder_filter_index",
        "level_filter",
        "search_string",
        "sort_index",
        "sorts",
        "variant",
      ],
      properties: {
        filters: {
          type: "array",
          items: {
            $ref: "#/definitions/SongFilterType",
          },
        },
        folder_filter_index: {
          type: "integer",
          format: "uint",
          minimum: 0.0,
        },
        level_filter: {
          type: "integer",
          format: "uint8",
          minimum: 0.0,
        },
        search_string: {
          type: "string",
        },
        sort_index: {
          type: "integer",
          format: "uint",
          minimum: 0.0,
        },
        sorts: {
          type: "array",
          items: {
            $ref: "#/definitions/SongSort",
          },
        },
        variant: {
          type: "string",
          enum: ["SongSelect"],
        },
      },
    },
  ],
  definitions: {
    SongFilterType: {
      oneOf: [
        {
          type: "string",
          enum: ["None"],
        },
        {
          type: "object",
          required: ["Folder"],
          properties: {
            Folder: {
              type: "string",
            },
          },
          additionalProperties: false,
        },
        {
          type: "object",
          required: ["Collection"],
          properties: {
            Collection: {
              type: "string",
            },
          },
          additionalProperties: false,
        },
      ],
    },
    SongSort: {
      type: "object",
      required: ["direction", "sort_type"],
      properties: {
        direction: {
          $ref: "#/definitions/SortDir",
        },
        sort_type: {
          $ref: "#/definitions/SongSortType",
        },
      },
    },
    SongSortType: {
      type: "string",
      enum: ["Title", "Score", "Date", "Artist", "Effector"],
    },
    SortDir: {
      type: "string",
      enum: ["Asc", "Desc"],
    },
  },
} as const;

const client = {
  $schema: "http://json-schema.org/draft-07/schema#",
  title: "ClientEvent",
  oneOf: [
    {
      type: "object",
      required: ["v", "variant"],
      properties: {
        v: {
          type: "string",
        },
        variant: {
          type: "string",
          enum: ["Invalid"],
        },
      },
    },
    {
      type: "object",
      required: ["variant"],
      properties: {
        variant: {
          type: "string",
          enum: ["Start"],
        },
      },
    },
    {
      type: "object",
      required: ["variant"],
      properties: {
        variant: {
          type: "string",
          enum: ["Back"],
        },
      },
    },
    {
      type: "object",
      required: ["v", "variant"],
      properties: {
        v: {
          type: "string",
        },
        variant: {
          type: "string",
          enum: ["SetSearch"],
        },
      },
    },
    {
      type: "object",
      required: ["v", "variant"],
      properties: {
        v: {
          type: "integer",
          format: "uint8",
          minimum: 0.0,
        },
        variant: {
          type: "string",
          enum: ["SetLevelFilter"],
        },
      },
    },
    {
      type: "object",
      required: ["v", "variant"],
      properties: {
        v: {
          $ref: "#/definitions/SongFilterType",
        },
        variant: {
          type: "string",
          enum: ["SetSongFilterType"],
        },
      },
    },
    {
      type: "object",
      required: ["v", "variant"],
      properties: {
        v: {
          $ref: "#/definitions/SongSort",
        },
        variant: {
          type: "string",
          enum: ["SetSongSort"],
        },
      },
    },
  ],
  definitions: {
    SongFilterType: {
      oneOf: [
        {
          type: "string",
          enum: ["None"],
        },
        {
          type: "object",
          required: ["Folder"],
          properties: {
            Folder: {
              type: "string",
            },
          },
          additionalProperties: false,
        },
        {
          type: "object",
          required: ["Collection"],
          properties: {
            Collection: {
              type: "string",
            },
          },
          additionalProperties: false,
        },
      ],
    },
    SongSort: {
      type: "object",
      required: ["direction", "sort_type"],
      properties: {
        direction: {
          $ref: "#/definitions/SortDir",
        },
        sort_type: {
          $ref: "#/definitions/SongSortType",
        },
      },
    },
    SongSortType: {
      type: "string",
      enum: ["Title", "Score", "Date", "Artist", "Effector"],
    },
    SortDir: {
      type: "string",
      enum: ["Asc", "Desc"],
    },
  },
} as const;

export type GameState = FromSchema<typeof server>;
export type ClientMessage = FromSchema<typeof client>;
