/*!

A `BufferTexture` is a special kind of one-dimensional texture that gets its data from a buffer.
Buffer textures have very limited capabilities (you can't draw to them for example). They are an
alternative to uniform buffers and SSBOs.


*/
use std::mem;
use std::ops::{Deref, DerefMut};

use gl;
use version::Version;
use version::Api;
use backend::Facade;
use ContextExt;

use BufferViewExt;
use buffer::BufferMode;
use buffer::BufferType;
use buffer::BufferView;
use buffer::BufferCreationError;
use buffer::Content as BufferContent;

/// Error that can happen while building the texture part of a buffer texture.
#[derive(Copy, Clone, Debug)]
pub enum TextureCreationError {
    /// Buffer textures are not supported at all.
    NotSupported,

    /// The requested format is not supported in combination with the given texture buffer type.
    FormatNotSupported,

    /// The size of the buffer that you are trying to bind exceeds `GL_MAX_TEXTURE_BUFFER_SIZE`.
    TooLarge,
}

/// Error that can happen while building a buffer texture.
#[derive(Copy, Clone, Debug)]
pub enum CreationError {
    /// Failed to create the buffer.
    BufferCreationError(BufferCreationError),

    /// Failed to create the texture.
    TextureCreationError(TextureCreationError),
}

impl From<BufferCreationError> for CreationError {
    fn from(err: BufferCreationError) -> CreationError {
        CreationError::BufferCreationError(err)
    }
}

impl From<TextureCreationError> for CreationError {
    fn from(err: TextureCreationError) -> CreationError {
        CreationError::TextureCreationError(err)
    }
}

/// Type of a buffer texture.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BufferTextureType {
    /// The texture will behave as if it contained floating-point data. It can be sampled with
    /// a `samplerBuffer` in your GLSL code.
    ///
    /// If the buffer actually contains integer values, they will be normalized so that `0`
    /// is interpreted as `0.0` and the maximum possible value (for example `255` for `u8`s)
    /// is interpreted as `1.0`.
    Float,

    /// The texture will behave as if it contained signed integral data. It can be sampled with
    /// a `isamplerBuffer` in your GLSL code.
    Integral,

    /// The texture will behave as if it contained unsigned integral data. It can be sampled with
    /// a `usamplerBuffer` in your GLSL code.
    Unsigned,
}

/// A one-dimensional texture that gets its data from a buffer.
pub struct BufferTexture<T> where [T]: BufferContent {
    buffer: BufferView<[T]>,
    texture: gl::types::GLuint,
    ty: BufferTextureType,
}

impl<T> BufferTexture<T> where [T]: BufferContent, T: TextureBufferContent + Copy {
    /// Builds a new texture buffer from data.
    pub fn new<F>(facade: &F, data: &[T], ty: BufferTextureType)
                  -> Result<BufferTexture<T>, CreationError>
                  where F: Facade
    {
        BufferTexture::new_impl(facade, data, BufferMode::Default, ty)
    }

    /// Builds a new texture buffer from data.
    pub fn dynamic<F>(facade: &F, data: &[T], ty: BufferTextureType)
                  -> Result<BufferTexture<T>, CreationError>
                      where F: Facade
    {
        BufferTexture::new_impl(facade, data, BufferMode::Dynamic, ty)
    }

    /// Builds a new texture buffer from data.
    pub fn persistent<F>(facade: &F, data: &[T], ty: BufferTextureType)
                  -> Result<BufferTexture<T>, CreationError>
                         where F: Facade
    {
        BufferTexture::new_impl(facade, data, BufferMode::Persistent, ty)
    }

    /// Builds a new texture buffer from data.
    pub fn immutable<F>(facade: &F, data: &[T], ty: BufferTextureType)
                        -> Result<BufferTexture<T>, CreationError>
                        where F: Facade
    {
        BufferTexture::new_impl(facade, data, BufferMode::Immutable, ty)
    }

    fn new_impl<F>(facade: &F, data: &[T], mode: BufferMode, ty: BufferTextureType)
                   -> Result<BufferTexture<T>, CreationError>
                   where F: Facade
    {
        let buffer = try!(BufferView::new(facade, data, BufferType::TextureBuffer, mode));
        BufferTexture::from_buffer(facade, buffer, ty).map_err(|(e, _)| e.into())
    }

    /// Builds a new empty buffer buffer.
    pub fn empty<F>(facade: &F, len: usize, ty: BufferTextureType)
                    -> Result<BufferTexture<T>, CreationError>
                    where F: Facade
    {
        BufferTexture::empty_impl(facade, len, ty, BufferMode::Default)
    }

    /// Builds a new empty buffer buffer.
    pub fn empty_dynamic<F>(facade: &F, len: usize, ty: BufferTextureType)
                            -> Result<BufferTexture<T>, CreationError>
                            where F: Facade
    {
        BufferTexture::empty_impl(facade, len, ty, BufferMode::Dynamic)
    }

    /// Builds a new empty buffer buffer.
    pub fn empty_persistent<F>(facade: &F, len: usize, ty: BufferTextureType)
                               -> Result<BufferTexture<T>, CreationError>
                               where F: Facade
    {
        BufferTexture::empty_impl(facade, len, ty, BufferMode::Persistent)
    }

    /// Builds a new empty buffer buffer.
    pub fn empty_immutable<F>(facade: &F, len: usize, ty: BufferTextureType)
                              -> Result<BufferTexture<T>, CreationError>
                              where F: Facade
    {
        BufferTexture::empty_impl(facade, len, ty, BufferMode::Immutable)
    }

    fn empty_impl<F>(facade: &F, len: usize, ty: BufferTextureType, mode: BufferMode)
                     -> Result<BufferTexture<T>, CreationError>
                     where F: Facade
    {
        let buffer = try!(BufferView::empty_array(facade, BufferType::TextureBuffer, len, mode));
        BufferTexture::from_buffer(facade, buffer, ty).map_err(|(e, _)| e.into())
    }

    /// Builds a new buffer texture by taking ownership of a buffer.
    pub fn from_buffer<F>(context: &F, buffer: BufferView<[T]>, ty: BufferTextureType)
                          -> Result<BufferTexture<T>, (TextureCreationError, BufferView<[T]>)>
                          where F: Facade
    {
        let context = context.get_context();
        let mut ctxt = context.make_current();

        // before starting, we determine the internal format and check that buffer textures are
        // supported
        let internal_format = if ctxt.version >= &Version(Api::Gl, 3, 0) ||
                                 ctxt.extensions.gl_oes_texture_buffer ||
                                 ctxt.extensions.gl_ext_texture_buffer
        {
            match (T::get_type(), ty) {
                (TextureBufferContentType::U8, BufferTextureType::Float) => gl::R8,
                (TextureBufferContentType::U8, BufferTextureType::Unsigned) => gl::R8UI,
                (TextureBufferContentType::I8, BufferTextureType::Integral) => gl::R8I,
                (TextureBufferContentType::U16, BufferTextureType::Float) => gl::R16,
                (TextureBufferContentType::U16, BufferTextureType::Unsigned) => gl::R16UI,
                (TextureBufferContentType::I16, BufferTextureType::Integral) => gl::R16I,
                (TextureBufferContentType::U32, BufferTextureType::Unsigned) => gl::R32UI,
                (TextureBufferContentType::I32, BufferTextureType::Integral) => gl::R32I,
                (TextureBufferContentType::U8U8, BufferTextureType::Float) => gl::RG8,
                (TextureBufferContentType::U8U8, BufferTextureType::Unsigned) => gl::RG8UI,
                (TextureBufferContentType::I8I8, BufferTextureType::Integral) => gl::RG8I,
                (TextureBufferContentType::U16U16, BufferTextureType::Float) => gl::RG16,
                (TextureBufferContentType::U16U16, BufferTextureType::Unsigned) => gl::RG16UI,
                (TextureBufferContentType::I16I16, BufferTextureType::Integral) => gl::RG16I,
                (TextureBufferContentType::U32U32, BufferTextureType::Unsigned) => gl::RG32UI,
                (TextureBufferContentType::I32I32, BufferTextureType::Integral) => gl::RG32I,
                (TextureBufferContentType::U8U8U8U8, BufferTextureType::Float) => gl::RGBA8,
                (TextureBufferContentType::U8U8U8U8, BufferTextureType::Unsigned) => gl::RGBA8UI,
                (TextureBufferContentType::I8I8I8I8, BufferTextureType::Integral) => gl::RGBA8I,
                (TextureBufferContentType::U16U16U16U16, BufferTextureType::Float) => gl::RGBA16,
                (TextureBufferContentType::U16U16U16U16, BufferTextureType::Unsigned) => 
                                                                                      gl::RGBA16UI,
                (TextureBufferContentType::I16I16I16I16, BufferTextureType::Integral) => 
                                                                                       gl::RGBA16I,
                (TextureBufferContentType::U32U32U32U32, BufferTextureType::Unsigned) => 
                                                                                      gl::RGBA32UI,
                (TextureBufferContentType::I32I32I32I32, BufferTextureType::Integral) => 
                                                                                       gl::RGBA32I,
                (TextureBufferContentType::F16, BufferTextureType::Float) => gl::R16F,
                (TextureBufferContentType::F32, BufferTextureType::Float) => gl::R32F,
                (TextureBufferContentType::F16F16, BufferTextureType::Float) => gl::RG16F,
                (TextureBufferContentType::F32F32, BufferTextureType::Float) => gl::RG32F,
                (TextureBufferContentType::F16F16F16F16, BufferTextureType::Float) => gl::RGBA16F,
                (TextureBufferContentType::F32F32F32F32, BufferTextureType::Float) => gl::RGBA32F,

                (TextureBufferContentType::U32U32U32, BufferTextureType::Unsigned)
                                            if ctxt.version >= &Version(Api::Gl, 4, 0) ||
                                               ctxt.extensions.gl_arb_texture_buffer_object_rgb32
                                                                                    => gl::RGB32UI,
                (TextureBufferContentType::I32I32I32, BufferTextureType::Integral)
                                            if ctxt.version >= &Version(Api::Gl, 4, 0) ||
                                               ctxt.extensions.gl_arb_texture_buffer_object_rgb32
                                                                                    => gl::RGB32I,
                (TextureBufferContentType::F32F32F32, BufferTextureType::Float)
                                            if ctxt.version >= &Version(Api::Gl, 4, 0) ||
                                               ctxt.extensions.gl_arb_texture_buffer_object_rgb32
                                                                                    => gl::RGB32F,

                _ => return Err((TextureCreationError::FormatNotSupported, buffer))
            }

        } else if ctxt.extensions.gl_arb_texture_buffer_object ||
                  ctxt.extensions.gl_ext_texture_buffer_object
        {
            match (T::get_type(), ty) {
                (TextureBufferContentType::U8U8U8U8, BufferTextureType::Float) => gl::RGBA8,
                (TextureBufferContentType::U16U16U16U16, BufferTextureType::Float) => gl::RGBA16,
                (TextureBufferContentType::F16F16F16F16, BufferTextureType::Float) => gl::RGBA16F,
                (TextureBufferContentType::F32F32F32F32, BufferTextureType::Float) => gl::RGBA32F,
                (TextureBufferContentType::I8I8I8I8, BufferTextureType::Integral) => gl::RGBA8I,
                (TextureBufferContentType::I16I16I16I16, BufferTextureType::Integral) =>
                                                                                      gl::RGBA16I,
                (TextureBufferContentType::I32I32I32I32, BufferTextureType::Integral) =>
                                                                                      gl::RGBA32I,
                (TextureBufferContentType::U8U8U8U8, BufferTextureType::Unsigned) => gl::RGBA8UI,
                (TextureBufferContentType::U16U16U16U16, BufferTextureType::Unsigned) =>
                                                                                      gl::RGBA16UI,
                (TextureBufferContentType::U32U32U32U32, BufferTextureType::Unsigned) =>
                                                                                      gl::RGBA32UI,

                // TODO: intensity?

                _ => return Err((TextureCreationError::FormatNotSupported, buffer))
            }

        } else {
            return Err((TextureCreationError::NotSupported, buffer));
        };

        // FIXME: check `TooLarge` error

        // TODO: use DSA if available

        // reserving the ID
        let id = unsafe {
            let mut id = mem::uninitialized();
            ctxt.gl.GenTextures(1, &mut id);
            id
        };

        // binding the texture
        unsafe {
            ctxt.gl.BindTexture(gl::TEXTURE_BUFFER, id);
            let act = ctxt.state.active_texture as usize;
            ctxt.state.texture_units[act].texture = id;
        }

        // binding the buffer
        debug_assert_eq!(buffer.get_offset_bytes(), 0);
        if ctxt.version >= &Version(Api::Gl, 3, 0) {
            unsafe {
                ctxt.gl.TexBuffer(gl::TEXTURE_BUFFER, internal_format, buffer.get_buffer_id());
            }
        } else if ctxt.extensions.gl_arb_texture_buffer_object {
            unsafe {
                ctxt.gl.TexBufferARB(gl::TEXTURE_BUFFER, internal_format, buffer.get_buffer_id());
            }
        } else if ctxt.extensions.gl_ext_texture_buffer_object ||
                  ctxt.extensions.gl_ext_texture_buffer
        {
            unsafe {
                ctxt.gl.TexBufferEXT(gl::TEXTURE_BUFFER, internal_format, buffer.get_buffer_id());
            }
        } else if ctxt.extensions.gl_oes_texture_buffer {
            unsafe {
                ctxt.gl.TexBufferOES(gl::TEXTURE_BUFFER, internal_format, buffer.get_buffer_id());
            }
        } else {
            // handled above ; note that this will leak the texture
            unreachable!();
        }

        Ok(BufferTexture {
            buffer: buffer,
            ty: ty,
            texture: id,
        })
    }
}

impl<T> Deref for BufferTexture<T> where [T]: BufferContent {
    type Target = BufferView<[T]>;

    fn deref(&self) -> &BufferView<[T]> {
        &self.buffer
    }
}

impl<T> DerefMut for BufferTexture<T> where [T]: BufferContent {
    fn deref_mut(&mut self) -> &mut BufferView<[T]> {
        &mut self.buffer
    }
}

impl<T> Drop for BufferTexture<T> where [T]: BufferContent {
    fn drop(&mut self) {
        let mut ctxt = self.buffer.get_context().make_current();

        // resetting the bindings
        for tex_unit in ctxt.state.texture_units.iter_mut() {
            if tex_unit.texture == self.texture {
                tex_unit.texture = 0;
            }
        }

        unsafe { ctxt.gl.DeleteTextures(1, [ self.texture ].as_ptr()); }
    }
}

///
///
/// Note that some three-component types are missing. This is not a mistake. OpenGL doesn't
/// support them.
#[allow(missing_docs)]
pub enum TextureBufferContentType {
    U8,
    I8,
    U16,
    I16,
    U32,
    I32,
    U8U8,
    I8I8,
    U16U16,
    I16I16,
    U32U32,
    I32I32,
    U32U32U32,
    I32I32I32,
    U8U8U8U8,
    I8I8I8I8,
    U16U16U16U16,
    I16I16I16I16,
    U32U32U32U32,
    I32I32I32I32,
    F16,
    F32,
    F16F16,
    F32F32,
    F32F32F32,
    F16F16F16F16,
    F32F32F32F32,
}

/// Trait for data types that can be interpreted by a buffer texture.
pub unsafe trait TextureBufferContent: BufferContent {
    /// Returns the enumeration corresponding to elements of this data type.
    fn get_type() -> TextureBufferContentType;
}

unsafe impl TextureBufferContent for u8 {
    fn get_type() -> TextureBufferContentType {
        TextureBufferContentType::U8
    }
}

unsafe impl TextureBufferContent for i8 {
    fn get_type() -> TextureBufferContentType {
        TextureBufferContentType::I8
    }
}

unsafe impl TextureBufferContent for u16 {
    fn get_type() -> TextureBufferContentType {
        TextureBufferContentType::U16
    }
}

unsafe impl TextureBufferContent for i16 {
    fn get_type() -> TextureBufferContentType {
        TextureBufferContentType::I16
    }
}

unsafe impl TextureBufferContent for u32 {
    fn get_type() -> TextureBufferContentType {
        TextureBufferContentType::U32
    }
}

unsafe impl TextureBufferContent for i32 {
    fn get_type() -> TextureBufferContentType {
        TextureBufferContentType::I32
    }
}

unsafe impl TextureBufferContent for (u8, u8) {
    fn get_type() -> TextureBufferContentType {
        TextureBufferContentType::U8U8
    }
}

unsafe impl TextureBufferContent for (i8, i8) {
    fn get_type() -> TextureBufferContentType {
        TextureBufferContentType::I8I8
    }
}

unsafe impl TextureBufferContent for (u16, u16) {
    fn get_type() -> TextureBufferContentType {
        TextureBufferContentType::U16U16
    }
}

unsafe impl TextureBufferContent for (i16, i16) {
    fn get_type() -> TextureBufferContentType {
        TextureBufferContentType::I16I16
    }
}

unsafe impl TextureBufferContent for (u32, u32) {
    fn get_type() -> TextureBufferContentType {
        TextureBufferContentType::U32U32
    }
}

unsafe impl TextureBufferContent for (i32, i32) {
    fn get_type() -> TextureBufferContentType {
        TextureBufferContentType::I32I32
    }
}

unsafe impl TextureBufferContent for (u32, u32, u32) {
    fn get_type() -> TextureBufferContentType {
        TextureBufferContentType::U32U32U32
    }
}

unsafe impl TextureBufferContent for (i32, i32, i32) {
    fn get_type() -> TextureBufferContentType {
        TextureBufferContentType::I32I32I32
    }
}

unsafe impl TextureBufferContent for (u8, u8, u8, u8) {
    fn get_type() -> TextureBufferContentType {
        TextureBufferContentType::U8U8U8U8
    }
}

unsafe impl TextureBufferContent for (i8, i8, i8, i8) {
    fn get_type() -> TextureBufferContentType {
        TextureBufferContentType::I8I8I8I8
    }
}

unsafe impl TextureBufferContent for (u16, u16, u16, u16) {
    fn get_type() -> TextureBufferContentType {
        TextureBufferContentType::U16U16U16U16
    }
}

unsafe impl TextureBufferContent for (i16, i16, i16, i16) {
    fn get_type() -> TextureBufferContentType {
        TextureBufferContentType::I16I16I16I16
    }
}

unsafe impl TextureBufferContent for (u32, u32, u32, u32) {
    fn get_type() -> TextureBufferContentType {
        TextureBufferContentType::U32U32U32U32
    }
}

unsafe impl TextureBufferContent for (i32, i32, i32, i32) {
    fn get_type() -> TextureBufferContentType {
        TextureBufferContentType::I32I32I32I32
    }
}

unsafe impl TextureBufferContent for f32 {
    fn get_type() -> TextureBufferContentType {
        TextureBufferContentType::F32
    }
}

unsafe impl TextureBufferContent for (f32, f32) {
    fn get_type() -> TextureBufferContentType {
        TextureBufferContentType::F32F32
    }
}

unsafe impl TextureBufferContent for (f32, f32, f32) {
    fn get_type() -> TextureBufferContentType {
        TextureBufferContentType::F32F32F32
    }
}

unsafe impl TextureBufferContent for (f32, f32, f32, f32) {
    fn get_type() -> TextureBufferContentType {
        TextureBufferContentType::F32F32F32F32
    }
}
