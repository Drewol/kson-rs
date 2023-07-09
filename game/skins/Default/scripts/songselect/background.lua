--Horizontal alignment
TEXT_ALIGN_LEFT 	= 1;
TEXT_ALIGN_CENTER 	= 2;
TEXT_ALIGN_RIGHT 	= 4;
--Vertical alignment
TEXT_ALIGN_TOP 		= 8;
TEXT_ALIGN_MIDDLE	= 16;
TEXT_ALIGN_BOTTOM	= 32;
TEXT_ALIGN_BASELINE	= 64;
local r, g, b, a = game.GetSkinSetting("col_test")
local timer = 0.0
local bgMesh = gfx.CreateShadedMesh("songSelBack")
local verts = {}
local numVerts = 10
local vh = 1/numVerts
local seed = math.random() * 100
for y=0,numVerts do    
    local yp = y / numVerts
    for x=0,numVerts do 
        local xp = x / numVerts
        if y % 2 == 1 then 
            xp = (numVerts - x) / numVerts + vh / 2
            table.insert(verts, {{xp, yp}, {0, 0}})
            table.insert(verts, {{xp - vh / 2, yp + vh}, {0, 0}})
        else
            table.insert(verts, {{xp, yp}, {0, 0}})
            table.insert(verts, {{xp + vh / 2, yp + vh}, {0, 0}})
        end
    end
end

bgMesh:SetPrimitiveType(bgMesh.PRIM_TRISTRIP)
bgMesh:SetBlendMode(bgMesh.BLEND_ADD)
bgMesh:SetData(verts)
bgMesh:SetParam("seed", seed)
bgMesh:SetParamVec4("color", r / 255, g / 255, b / 255, a / 255)
render = function(deltaTime)
    timer = timer + deltaTime * 0.35
    resx,resy = game.GetResolution()
    bgMesh:SetParam("timer", timer)
    bgMesh:SetParam("scale", math.max(resx, resy))
    bgMesh:Draw()
    gfx.ForceRender()
end
